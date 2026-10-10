import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/desktop/widgets/quick_connect_bar.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('Enter connects directly and suppresses repeated launches', (tester) async {
    final controller = TextEditingController();
    final pending = Completer<void>();
    final calls = <String>[];
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: QuickConnectBar(
      controller: controller,
      addressLabel: 'Local Address',
      connectLabel: 'Connect',
      onConnect: (endpoint) { calls.add(endpoint); return pending.future; },
    ))));
    await tester.enterText(find.byType(TextField), '  192.168.1.20:21118  ');
    await tester.testTextInput.receiveAction(TextInputAction.go);
    await tester.pump();
    expect(calls, ['192.168.1.20:21118']);
    await tester.tap(find.byType(ElevatedButton));
    await tester.testTextInput.receiveAction(TextInputAction.go);
    expect(calls, hasLength(1));
    // Closing the page while its connection window is opening is safe.
    await tester.pumpWidget(const SizedBox.shrink());
    pending.complete();
    await tester.pump();
    expect(tester.takeException(), isNull);
    controller.dispose();
  });

  testWidgets('button ignores blank addresses and accepts VPN addresses', (tester) async {
    final controller = TextEditingController(text: '  ');
    final calls = <String>[];
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: QuickConnectBar(
      controller: controller,
      addressLabel: 'Local Address',
      connectLabel: 'Connect',
      onConnect: (endpoint) async { calls.add(endpoint); },
    ))));
    await tester.tap(find.byType(ElevatedButton));
    expect(calls, isEmpty);
    await tester.enterText(find.byType(TextField), '100.81.211.45:21118');
    await tester.tap(find.byType(ElevatedButton));
    await tester.pump();
    expect(calls, ['100.81.211.45:21118']);
    await tester.pumpWidget(const SizedBox.shrink());
    controller.dispose();
  });
}
