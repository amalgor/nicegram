import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/main.dart';
import 'package:hydra_mobile/src/rust/frb_generated.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => await RustLib.init());
  testWidgets('App starts with Hydra title', (WidgetTester tester) async {
    await tester.pumpWidget(const HydraApp());
    expect(find.text('Hydra P2P Node'), findsOneWidget);
  });
}
