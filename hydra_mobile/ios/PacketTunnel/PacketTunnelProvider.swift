import Foundation
import NetworkExtension

@_silgen_name("hydra_extension_start")
private func hydra_extension_start(
  _ baseDir: UnsafePointer<CChar>,
  _ tunFd: Int32
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("hydra_extension_stop")
private func hydra_extension_stop() -> UnsafeMutablePointer<CChar>?

@_silgen_name("hydra_extension_apply_command")
private func hydra_extension_apply_command(
  _ commandJson: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("hydra_extension_snapshot_json")
private func hydra_extension_snapshot_json() -> UnsafeMutablePointer<CChar>?

@_silgen_name("hydra_extension_string_free")
private func hydra_extension_string_free(_ ptr: UnsafeMutablePointer<CChar>?)

private let hydraAppGroupId = "group.com.hydra.network.hydraMobile.shared"

final class PacketTunnelProvider: NEPacketTunnelProvider {
  override func startTunnel(
    options: [String: NSObject]?,
    completionHandler: @escaping (Error?) -> Void
  ) {
    let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "10.0.0.2")
    let ipv4 = NEIPv4Settings(addresses: ["10.0.0.2"], subnetMasks: ["255.255.255.0"])
    ipv4.includedRoutes = [NEIPv4Route.default()]
    ipv4.excludedRoutes = [NEIPv4Route(destinationAddress: "127.0.0.0", subnetMask: "255.0.0.0")]
    settings.ipv4Settings = ipv4
    settings.dnsSettings = NEDNSSettings(servers: ["8.8.8.8"])
    settings.mtu = 1500 as NSNumber

    setTunnelNetworkSettings(settings) { error in
      if let error {
        completionHandler(error)
        return
      }

      let baseDir = self.resolveBaseDir(from: options)
      guard let tunFd = self.packetFlowFileDescriptor() else {
        completionHandler(
          NSError(
            domain: "HydraPacketTunnel",
            code: 1,
            userInfo: [NSLocalizedDescriptionKey: "Unable to resolve packet flow file descriptor."]
          )
        )
        return
      }

      baseDir.withCString { cBaseDir in
        if let errorPtr = hydra_extension_start(cBaseDir, tunFd) {
          let message = self.takeRustString(errorPtr)
          completionHandler(
            NSError(
              domain: "HydraPacketTunnel",
              code: 2,
              userInfo: [NSLocalizedDescriptionKey: message]
            )
          )
        } else {
          completionHandler(nil)
        }
      }
    }
  }

  override func stopTunnel(
    with reason: NEProviderStopReason,
    completionHandler: @escaping () -> Void
  ) {
    if let errorPtr = hydra_extension_stop() {
      _ = takeRustString(errorPtr)
    }
    completionHandler()
  }

  override func handleAppMessage(
    _ messageData: Data,
    completionHandler: ((Data?) -> Void)? = nil
  ) {
    guard let command = String(data: messageData, encoding: .utf8) else {
      completionHandler?(nil)
      return
    }

    command.withCString { cCommand in
      if let errorPtr = hydra_extension_apply_command(cCommand) {
        let message = self.takeRustString(errorPtr)
        completionHandler?(message.data(using: .utf8))
        return
      }

      if let snapshotPtr = hydra_extension_snapshot_json() {
        let snapshot = self.takeRustString(snapshotPtr)
        completionHandler?(snapshot.data(using: .utf8))
      } else {
        completionHandler?(nil)
      }
    }
  }

  private func resolveBaseDir(from options: [String: NSObject]?) -> String {
    if let baseDir = options?["baseDir"] as? String {
      return baseDir
    }

    return FileManager.default
      .containerURL(forSecurityApplicationGroupIdentifier: hydraAppGroupId)?
      .path ?? NSTemporaryDirectory()
  }

  private func packetFlowFileDescriptor() -> Int32? {
    if let fd = packetFlow.value(forKeyPath: "socket.fileDescriptor") as? Int32 {
      return fd
    }
    if let fd = packetFlow.value(forKeyPath: "_socket.fileDescriptor") as? Int32 {
      return fd
    }
    return nil
  }

  private func takeRustString(_ ptr: UnsafeMutablePointer<CChar>) -> String {
    let string = String(cString: ptr)
    hydra_extension_string_free(ptr)
    return string
  }
}
