import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('peer avatar URLs use the fallback without loading a network image',
      (tester) async {
    const fallback = Text('device initials');
    for (final avatar in [
      'https://attacker.example/tracking.png',
      'http://127.0.0.1:21118/private',
      'https://192.168.1.1/admin',
      ' https://attacker.example/tracking.png ',
      '//attacker.example/tracking.png',
    ]) {
      final widget = buildAvatarWidget(
        avatar: avatar,
        size: 40,
        fallback: fallback,
      );
      expect(widget, same(fallback));
      await tester.pumpWidget(MaterialApp(home: widget));
      expect(find.byType(Image), findsNothing);
      expect(tester.takeException(), isNull);
    }
  });

  test('inline avatars remain available and invalid data uses the fallback', () {
    const fallback = Text('device initials');
    final bytes = [1, 2, 3];
    final widget = buildAvatarWidget(
      avatar: 'data:image/png;base64,${base64Encode(bytes)}',
      size: 40,
      fallback: fallback,
    ) as ClipRRect;
    final image = widget.child as Image;
    expect(image.image, isA<MemoryImage>());
    expect((image.image as MemoryImage).bytes, bytes);
    for (final avatar in ['', 'data:image/png;base64,%%%']) {
      expect(buildAvatarWidget(avatar: avatar, size: 40, fallback: fallback),
          same(fallback));
    }
  });
}
