import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/main.dart';

void main() {
  testWidgets('App renders main screen', (WidgetTester tester) async {
    await tester.pumpWidget(const HydraApp());
    expect(find.text('Hydra P2P Node'), findsOneWidget);
  });
}
