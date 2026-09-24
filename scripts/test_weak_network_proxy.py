import argparse
import asyncio
import time
import unittest
from weak_network_proxy import pump, private_address


class ProxyTests(unittest.IsolatedAsyncioTestCase):
    async def test_rate_limited_loopback_preserves_bytes(self):
        payload = bytes(range(256)) * 256
        completed = asyncio.get_running_loop().create_future()
        async def connected(reader, writer):
            source = asyncio.StreamReader()
            source.feed_data(payload)
            source.feed_eof()
            try:
                await pump(source, writer, 1, 40, 0, 0, 17)
                completed.set_result(None)
            except Exception as error:
                completed.set_exception(error)
        server = await asyncio.start_server(connected, '127.0.0.1', 0)
        async with server:
            start = time.monotonic()
            reader, writer = await asyncio.open_connection('127.0.0.1', server.sockets[0].getsockname()[1])
            received = await asyncio.wait_for(reader.read(), timeout=5)
            self.assertEqual(received, payload)
            self.assertGreaterEqual(time.monotonic() - start, 0.55)
            writer.close()
            await writer.wait_closed()
            await completed

    def test_only_private_and_tailscale_targets(self):
        for address in ['100.81.211.45', '172.16.50.140', 'fd7a:115c:a1e0::1', '127.0.0.1']:
            self.assertEqual(private_address(address), address)
        for address in ['8.8.8.8', '100.128.0.1', '2001:4860:4860::8888']:
            with self.assertRaises(argparse.ArgumentTypeError):
                private_address(address)


if __name__ == '__main__':
    unittest.main()
