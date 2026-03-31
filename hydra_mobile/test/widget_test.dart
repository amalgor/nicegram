import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/main.dart';

void main() {
  testWidgets('App renders main screen', (WidgetTester tester) async {
    await tester.pumpWidget(
      const HydraApp(
        home: Scaffold(body: Center(child: Text('Hydra P2P Node'))),
      ),
    );
    expect(find.text('Hydra P2P Node'), findsOneWidget);
  });
}
