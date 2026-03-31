import Flutter
import Foundation
import NetworkExtension

private let hydraVpnChannelName = "com.hydra.network/vpn"
private let hydraAppGroupId = "group.com.hydra.network.hydraMobile.shared"
private let hydraPacketTunnelBundleId = "com.hydra.network.hydraMobile.PacketTunnel"

final class HydraVpnBridge {
  static let shared = HydraVpnBridge()

  private var methodChannel: FlutterMethodChannel?
  private let manager = HydraVpnManager()

  private init() {}

  func attach(to messenger: FlutterBinaryMessenger) {
    guard methodChannel == nil else {
      return
    }

    let channel = FlutterMethodChannel(name: hydraVpnChannelName, binaryMessenger: messenger)
    channel.setMethodCallHandler { [weak self] call, result in
      self?.handle(call: call, result: result)
    }
    methodChannel = channel
  }

  private func handle(call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "getSharedBaseDir":
      result(manager.sharedBaseDir())
    case "startVpn":
      manager.startVpn(completion: result)
    case "stopVpn":
      manager.stopVpn(completion: result)
    case "getVpnActive":
      manager.getVpnActive(completion: result)
    case "sendControlCommand":
      guard let command = call.arguments as? String else {
        result(
          FlutterError(
            code: "invalid_arguments",
            message: "Expected JSON string command payload.",
            details: nil
          )
        )
        return
      }
      manager.sendControlCommand(command, completion: result)
    default:
      result(FlutterMethodNotImplemented)
    }
  }
}

final class HydraVpnManager {
  func sharedBaseDir() -> String? {
    FileManager.default
      .containerURL(forSecurityApplicationGroupIdentifier: hydraAppGroupId)?
      .path
  }

  func startVpn(completion: @escaping FlutterResult) {
    loadOrCreateManager { result in
      switch result {
      case .failure(let error):
        completion(FlutterError(code: "vpn_start_failed", message: error.localizedDescription, details: nil))
      case .success(let manager):
        let session = manager.connection as? NETunnelProviderSession
        do {
          var options: [String: NSObject] = [:]
          if let baseDir = self.sharedBaseDir() {
            options["baseDir"] = baseDir as NSString
          }
          try session?.startVPNTunnel(options: options)
          completion(true)
        } catch {
          completion(FlutterError(code: "vpn_start_failed", message: error.localizedDescription, details: nil))
        }
      }
    }
  }

  func stopVpn(completion: @escaping FlutterResult) {
    loadOrCreateManager { result in
      switch result {
      case .failure(let error):
        completion(FlutterError(code: "vpn_stop_failed", message: error.localizedDescription, details: nil))
      case .success(let manager):
        manager.connection.stopVPNTunnel()
        completion(true)
      }
    }
  }

  func getVpnActive(completion: @escaping FlutterResult) {
    loadOrCreateManager { result in
      switch result {
      case .failure:
        completion(false)
      case .success(let manager):
        let status = manager.connection.status
        completion(
          status == .connected ||
          status == .connecting ||
          status == .reasserting
        )
      }
    }
  }

  func sendControlCommand(_ command: String, completion: @escaping FlutterResult) {
    loadOrCreateManager { result in
      switch result {
      case .failure(let error):
        completion(
          FlutterError(code: "vpn_message_failed", message: error.localizedDescription, details: nil)
        )
      case .success(let manager):
        guard let session = manager.connection as? NETunnelProviderSession else {
          completion(
            FlutterError(code: "vpn_message_failed", message: "Missing provider session.", details: nil)
          )
          return
        }
        guard let payload = command.data(using: .utf8) else {
          completion(
            FlutterError(code: "vpn_message_failed", message: "Invalid UTF-8 command payload.", details: nil)
          )
          return
        }
        do {
          try session.sendProviderMessage(payload) { data in
            if let data, let response = String(data: data, encoding: .utf8), !response.isEmpty {
              completion(response)
            } else {
              completion(true)
            }
          }
        } catch {
          completion(
            FlutterError(code: "vpn_message_failed", message: error.localizedDescription, details: nil)
          )
        }
      }
    }
  }

  private func loadOrCreateManager(
    completion: @escaping (Result<NETunnelProviderManager, Error>) -> Void
  ) {
    NETunnelProviderManager.loadAllFromPreferences { managers, error in
      if let error {
        completion(.failure(error))
        return
      }

      let manager = managers?.first ?? NETunnelProviderManager()
      let proto = NETunnelProviderProtocol()
      proto.providerBundleIdentifier = hydraPacketTunnelBundleId
      proto.serverAddress = "Hydra Network"
      proto.providerConfiguration = ["appGroup": hydraAppGroupId]

      manager.protocolConfiguration = proto
      manager.localizedDescription = "Hydra Network"
      manager.isEnabled = true

      manager.saveToPreferences { error in
        if let error {
          completion(.failure(error))
          return
        }
        manager.loadFromPreferences { error in
          if let error {
            completion(.failure(error))
          } else {
            completion(.success(manager))
          }
        }
      }
    }
  }
}
