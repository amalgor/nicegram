import 'package:flutter/widgets.dart';
import 'package:hydra_mobile/app/proxy_controller.dart';

class ProxyScope extends InheritedNotifier<ProxyController> {
  const ProxyScope({super.key, required ProxyController controller, required super.child})
      : super(notifier: controller);

  /// Rebuilds the caller whenever the controller changes.
  static ProxyController of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<ProxyScope>()!.notifier!;

  /// For callbacks: no rebuild dependency.
  static ProxyController read(BuildContext context) =>
      context.getInheritedWidgetOfExactType<ProxyScope>()!.notifier!;
}
