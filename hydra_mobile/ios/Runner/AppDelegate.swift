import AVFoundation
import Flutter
import Network
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  private var native: HydraNativeBridge?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    if let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "HydraNativeBridge") {
      native = HydraNativeBridge(messenger: registrar.messenger())
    }
  }
}

/// Native services for the Dart side, over the `hydra/native` channel.
///
/// Dart -> native: setKeepAlive, keepAliveState, shareFiles, openUrl, deviceInfo.
/// Native -> Dart: log(level, message), networkChanged(info).
final class HydraNativeBridge {
  private let channel: FlutterMethodChannel
  private let keepAlive = BackgroundKeepAlive()
  private let pathMonitor = NWPathMonitor()
  private var lastPathSummary: String?
  /// Main-queue copy of the newest path report; Dart pulls it on attach
  /// because the first report usually fires before Dart has a handler.
  private var lastPath: [String: Any]?
  private var backgroundTask: UIBackgroundTaskIdentifier = .invalid

  init(messenger: FlutterBinaryMessenger) {
    channel = FlutterMethodChannel(name: "hydra/native", binaryMessenger: messenger)
    channel.setMethodCallHandler { [weak self] call, result in
      self?.handle(call, result: result)
    }
    keepAlive.log = { [weak self] level, message in self?.log(level, message) }
    startPathMonitor()
    observeSystemEvents()
  }

  private func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any] ?? [:]
    switch call.method {
    case "setKeepAlive":
      let enabled = args["enabled"] as? Bool ?? false
      result(enabled ? keepAlive.start() : keepAlive.stop())
    case "networkState":
      result(lastPath)
    case "keepAliveState":
      result(keepAlive.isRunning)
    case "shareFiles":
      let paths = args["paths"] as? [String] ?? []
      share(paths: paths, text: args["text"] as? String, result: result)
    case "openUrl":
      guard let string = args["url"] as? String, let url = URL(string: string) else {
        result(false)
        return
      }
      UIApplication.shared.open(url, options: [:]) { opened in result(opened) }
    case "deviceInfo":
      let device = UIDevice.current
      let info = Bundle.main.infoDictionary ?? [:]
      result([
        "model": machineIdentifier(),
        "system": "\(device.systemName) \(device.systemVersion)",
        "appVersion": info["CFBundleShortVersionString"] as? String ?? "?",
        "appBuild": info["CFBundleVersion"] as? String ?? "?",
        "lowPowerMode": ProcessInfo.processInfo.isLowPowerModeEnabled,
      ])
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func log(_ level: String, _ message: String) {
    DispatchQueue.main.async {
      self.channel.invokeMethod("log", arguments: ["level": level, "message": message])
    }
  }

  private func startPathMonitor() {
    pathMonitor.pathUpdateHandler = { [weak self] path in
      guard let self else { return }
      var interfaces: [String] = []
      if path.usesInterfaceType(.wifi) { interfaces.append("wifi") }
      if path.usesInterfaceType(.cellular) { interfaces.append("cellular") }
      if path.usesInterfaceType(.wiredEthernet) { interfaces.append("ethernet") }
      if path.usesInterfaceType(.other) { interfaces.append("other") }
      let status: String
      switch path.status {
      case .satisfied: status = "satisfied"
      case .unsatisfied: status = "unsatisfied"
      case .requiresConnection: status = "requiresConnection"
      @unknown default: status = "unknown"
      }
      let summary = "\(status) [\(interfaces.joined(separator: ","))]"
      let changed = self.lastPathSummary != nil && self.lastPathSummary != summary
      self.lastPathSummary = summary
      let report: [String: Any] = [
        "status": status,
        "interfaces": interfaces,
        "expensive": path.isExpensive,
        "constrained": path.isConstrained,
        "changed": changed,
      ]
      DispatchQueue.main.async {
        self.lastPath = report
        self.channel.invokeMethod("networkChanged", arguments: report)
      }
    }
    pathMonitor.start(queue: DispatchQueue(label: "hydra.path-monitor"))
  }

  private func observeSystemEvents() {
    let center = NotificationCenter.default
    center.addObserver(forName: UIApplication.didReceiveMemoryWarningNotification, object: nil, queue: .main) { [weak self] _ in
      self?.log("WARN", "iOS memory warning")
    }
    center.addObserver(forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main) { [weak self] _ in
      guard let self else { return }
      // Short grace period even without keep-alive, so in-flight requests finish.
      self.backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "hydra-proxy") { [weak self] in
        guard let self else { return }
        self.log("WARN", "iOS background time expired; the app will be suspended unless keep-alive audio is running")
        UIApplication.shared.endBackgroundTask(self.backgroundTask)
        self.backgroundTask = .invalid
      }
      let remaining = UIApplication.shared.backgroundTimeRemaining
      self.log("INFO", "Entered background (keepAlive=\(self.keepAlive.isRunning), backgroundTimeRemaining=\(remaining > 1e6 ? "unlimited" : String(format: "%.0fs", remaining))")
    }
    center.addObserver(forName: UIApplication.willEnterForegroundNotification, object: nil, queue: .main) { [weak self] _ in
      guard let self else { return }
      if self.backgroundTask != .invalid {
        UIApplication.shared.endBackgroundTask(self.backgroundTask)
        self.backgroundTask = .invalid
      }
      self.log("INFO", "Entering foreground (keepAlive=\(self.keepAlive.isRunning))")
    }
    center.addObserver(forName: UIApplication.willTerminateNotification, object: nil, queue: .main) { [weak self] _ in
      self?.log("WARN", "iOS is terminating the app")
    }
    center.addObserver(forName: Notification.Name.NSProcessInfoPowerStateDidChange, object: nil, queue: .main) { [weak self] _ in
      self?.log("INFO", "Low Power Mode: \(ProcessInfo.processInfo.isLowPowerModeEnabled)")
    }
  }

  private func share(paths: [String], text: String?, result: @escaping FlutterResult) {
    var items: [Any] = paths
      .filter { FileManager.default.fileExists(atPath: $0) }
      .map { URL(fileURLWithPath: $0) }
    if let text { items.insert(text, at: 0) }
    guard !items.isEmpty, let presenter = topViewController() else {
      result(false)
      return
    }
    let controller = UIActivityViewController(activityItems: items, applicationActivities: nil)
    if let popover = controller.popoverPresentationController {
      popover.sourceView = presenter.view
      popover.sourceRect = CGRect(x: presenter.view.bounds.midX, y: presenter.view.bounds.maxY - 80, width: 1, height: 1)
    }
    controller.completionWithItemsHandler = { _, completed, _, _ in result(completed) }
    presenter.present(controller, animated: true)
  }

  private func topViewController() -> UIViewController? {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    let window = scenes.flatMap(\.windows).first { $0.isKeyWindow } ?? scenes.first?.windows.first
    var top = window?.rootViewController
    while let presented = top?.presentedViewController { top = presented }
    return top
  }

  private func machineIdentifier() -> String {
    var info = utsname()
    uname(&info)
    return withUnsafeBytes(of: &info.machine) { raw in
      String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
    }
  }
}

/// Keeps the process running while in the background by playing silence
/// (`UIBackgroundModes: audio`). Without it iOS suspends the app seconds
/// after the user switches to another app, and the local SOCKS5 port stops
/// answering. Mixes with other audio, so it never interrupts music or calls.
final class BackgroundKeepAlive {
  var log: ((String, String) -> Void)?
  private var engine = AVAudioEngine()
  private var player = AVAudioPlayerNode()
  private var wanted = false
  private var configured = false

  var isRunning: Bool { wanted && engine.isRunning }

  init() {
    let center = NotificationCenter.default
    center.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] note in
      self?.handleInterruption(note)
    }
    center.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: nil, queue: .main) { [weak self] _ in
      guard let self, self.wanted else { return }
      self.log?("WARN", "Keep-alive: media services were reset, restarting audio")
      self.engine = AVAudioEngine()
      self.player = AVAudioPlayerNode()
      self.configured = false
      _ = self.start()
    }
    center.addObserver(forName: .AVAudioEngineConfigurationChange, object: nil, queue: .main) { [weak self] note in
      guard let self, self.wanted, (note.object as AnyObject?) === self.engine else { return }
      self.log?("INFO", "Keep-alive: audio configuration changed, restarting audio")
      _ = self.start()
    }
  }

  func start() -> Bool {
    wanted = true
    do {
      let session = AVAudioSession.sharedInstance()
      try session.setCategory(.playback, mode: .default, options: [.mixWithOthers])
      try session.setActive(true)
      if !configured {
        let format = AVAudioFormat(standardFormatWithSampleRate: 44_100, channels: 1)!
        engine.attach(player)
        engine.connect(player, to: engine.mainMixerNode, format: format)
        engine.mainMixerNode.outputVolume = 0
        configured = true
      }
      if !engine.isRunning {
        try engine.start()
      }
      if !player.isPlaying {
        let format = player.outputFormat(forBus: 0)
        let frames = AVAudioFrameCount(format.sampleRate)
        if let silence = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) {
          silence.frameLength = frames
          if let channels = silence.floatChannelData {
            for index in 0..<Int(format.channelCount) {
              channels[index].update(repeating: 0, count: Int(frames))
            }
          }
          player.scheduleBuffer(silence, at: nil, options: .loops)
        }
        player.play()
      }
      log?("INFO", "Keep-alive: background audio running")
      return true
    } catch {
      log?("ERROR", "Keep-alive: could not start background audio: \(error.localizedDescription)")
      return false
    }
  }

  func stop() -> Bool {
    wanted = false
    player.stop()
    engine.stop()
    try? AVAudioSession.sharedInstance().setActive(false, options: [.notifyOthersOnDeactivation])
    log?("INFO", "Keep-alive: background audio stopped")
    return false
  }

  private func handleInterruption(_ note: Notification) {
    guard let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
          let type = AVAudioSession.InterruptionType(rawValue: raw) else { return }
    switch type {
    case .began:
      log?("WARN", "Keep-alive: audio interrupted (call/Siri/other app); the app may be suspended until it ends")
    case .ended:
      log?("INFO", "Keep-alive: audio interruption ended")
      if wanted { _ = start() }
    @unknown default:
      break
    }
  }
}
