import 'package:flutter/material.dart';

class QuickConnectBar extends StatefulWidget {
  const QuickConnectBar({
    super.key,
    required this.controller,
    required this.onConnect,
    required this.addressLabel,
    required this.connectLabel,
  });

  final TextEditingController controller;
  final Future<void> Function(String endpoint) onConnect;
  final String addressLabel;
  final String connectLabel;

  @override
  State<QuickConnectBar> createState() => _QuickConnectBarState();
}

class _QuickConnectBarState extends State<QuickConnectBar> {
  bool _connecting = false;

  Future<void> _connect() async {
    final endpoint = widget.controller.text.trim();
    if (_connecting || endpoint.isEmpty) return;
    setState(() => _connecting = true);
    try {
      await widget.onConnect(endpoint);
    } finally {
      if (mounted) setState(() => _connecting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: TextField(
            controller: widget.controller,
            autocorrect: false,
            enableSuggestions: false,
            textInputAction: TextInputAction.go,
            onSubmitted: (_) => _connect(),
            decoration: InputDecoration(
              labelText: widget.addressLabel,
              hintText: '192.168.1.20:21118',
              prefixIcon: const Icon(Icons.computer_outlined),
              border: const OutlineInputBorder(),
              isDense: true,
            ),
          ),
        ),
        const SizedBox(width: 12),
        ElevatedButton.icon(
          onPressed: _connecting ? null : _connect,
          icon: const Icon(Icons.arrow_forward),
          label: Text(widget.connectLabel),
        ),
      ],
    );
  }
}
