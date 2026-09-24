"""Local TCP impairment proxy for manual LAN/VPN desktop tests.

Example: python scripts/weak_network_proxy.py --target 192.168.1.20 --mbps 1
Connect SubnetDesk to 127.0.0.1:21120; verify the target's fingerprint as usual.
This proxy never decrypts application traffic. A stall simulates ordered TCP
retransmission delays; it does not claim to inject real IP packet loss.
"""
import argparse
import asyncio
import ipaddress
import random
import time


def private_address(value):
    address = ipaddress.ip_address(value)
    ranges = ('10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16',
              '100.64.0.0/10', '127.0.0.0/8', 'fc00::/7', '::1/128')
    if not any(address in ipaddress.ip_network(network) for network in ranges):
        raise argparse.ArgumentTypeError('Use a private or Tailscale IP address')
    return str(address)


async def pump(reader, writer, mbps, delay_ms, jitter_ms, stall_every, seed):
    rng = random.Random(seed)
    start = time.monotonic()
    pending = asyncio.Queue(maxsize=64)

    async def schedule():
        finish = start
        departure = start
        last_stall = 0
        try:
            while data := await reader.read(4096):
                now = time.monotonic()
                finish = max(finish, now) + len(data) * 8 / (mbps * 1_000_000)
                if stall_every > 0:
                    period = int((finish - start) / stall_every)
                    if period > last_stall:
                        last_stall = period
                        finish += 0.2
                propagation = max(0, delay_ms + rng.uniform(-jitter_ms, jitter_ms)) / 1000
                departure = max(departure, finish + propagation)
                await pending.put((departure, data))
        except Exception as error:
            await pending.put(error)
            return
        await pending.put(None)

    producer = asyncio.create_task(schedule())
    try:
        while True:
            item = await pending.get()
            if item is None:
                break
            if isinstance(item, Exception):
                raise item
            departure, data = item
            await asyncio.sleep(max(0, departure - time.monotonic()))
            writer.write(data)
            await writer.drain()
        await producer
    finally:
        producer.cancel()
        await asyncio.gather(producer, return_exceptions=True)
        writer.close()


async def serve(args):
    async def connected(reader, writer):
        tasks = []
        try:
            upstream, downstream = await asyncio.wait_for(
                asyncio.open_connection(args.target, args.target_port), timeout=5)
            tasks = [asyncio.create_task(pump(reader, downstream, args.mbps,
                     args.delay_ms, args.jitter_ms, args.stall_every, args.seed)),
                     asyncio.create_task(pump(upstream, writer, args.mbps,
                     args.delay_ms, args.jitter_ms, args.stall_every, args.seed + 1))]
            done, pending = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for task in pending:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            for task in done:
                if not task.cancelled() and task.exception():
                    print(f'Connection ended: {task.exception()}')
        except (OSError, asyncio.TimeoutError) as error:
            print(f'Connection failed: {error}')
        finally:
            for task in tasks:
                if not task.done():
                    task.cancel()
            writer.close()
    server = await asyncio.start_server(connected, '127.0.0.1', args.port, limit=8192)
    print(f'Listening on 127.0.0.1:{args.port}, target {args.target}:{args.target_port}')
    async with server:
        await server.serve_forever()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True, type=private_address)
    parser.add_argument('--target-port', type=int, default=21118)
    parser.add_argument('--port', type=int, default=21120)
    parser.add_argument('--mbps', type=float, default=1)
    parser.add_argument('--delay-ms', type=float, default=80)
    parser.add_argument('--jitter-ms', type=float, default=0)
    parser.add_argument('--stall-every', type=float, default=10)
    parser.add_argument('--seed', type=int, default=17)
    args = parser.parse_args()
    if args.mbps <= 0 or min(args.delay_ms, args.jitter_ms, args.stall_every) < 0:
        parser.error('Bandwidth must be positive and delays nonnegative')
    try:
        asyncio.run(serve(args))
    except KeyboardInterrupt:
        pass
