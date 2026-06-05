import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/main.dart';
import 'package:hydra_mobile/rust_init.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => await initHydraRustLib());
  testWidgets('App starts on connect screen', (WidgetTester tester) async {
    await tester.pumpWidget(const HydraApp());
    expect(find.text('Connect'), findsWidgets);
  });
}
