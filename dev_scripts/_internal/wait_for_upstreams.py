"""No dependency python for waiting for upstreams, so nginx doesn't return 502's before ready."""

import http.client
import os
import socket
import time

# Before starting nginx, we need to make sure all upstreams are available.
# This prevents 502 errors just before start.

# Most importantly when using e.g. fly.io and a new container is being spun up,
# this prevents the user being given a 502 error, instead of a slightly longer delay and the proper response returned.

# TODO need to do for py backend too
healthchecks: list[tuple[str, str, bool]] = []

max_seconds = 10
seconds_between_checks = 0.05
start = time.time()
responses: list[str] = []
success = False
is_first = True
conn_timeout = 0.05
while time.time() - start < max_seconds:
    responses.clear()
    all_succeeded = True
    for sock_or_port, path, is_sock in healthchecks:
        if is_sock:
            # Pretty low-level as don't want to use any external libraries.
            # Got the sock stuff from:
            # https://github.com/marverix/HTTPUnixSocketConnection/blob/master/httpunixsocketconnection/__init__.py

            conn: http.client.HTTPConnection
            if not os.path.exists(sock_or_port):
                all_succeeded = False
                responses.append("Socket '{}' does not exist.".format(sock_or_port))
                continue
            conn = http.client.HTTPConnection("localhost", timeout=conn_timeout)
            conn.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            conn.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            conn.sock.settimeout(conn_timeout)
            try:
                conn.sock.connect(sock_or_port)
            except ConnectionRefusedError:
                all_succeeded = False
                responses.append("Socket '{}' refused connection.".format(sock_or_port))
                continue
        else:
            conn = http.client.HTTPConnection(
                "localhost", port=int(sock_or_port), timeout=conn_timeout
            )
        conn.request("GET", path)
        response = conn.getresponse()
        if response.status != 200:
            all_succeeded = False
            responses.append(response.read().decode())

    if all_succeeded:
        success = True
        break
    if is_first:
        print("Waiting up to {} seconds for upstreams to be ready...".format(max_seconds))
        is_first = False
    time.sleep(seconds_between_checks)

if not success:

    def format_response(sock_or_port: str, path: str, is_sock: bool, response: str) -> str:
        """Format a response for printing."""
        return f"sock_or_port: {sock_or_port}, path: {path}, is_sock: {is_sock}, response: \n{response}\n\n".format(
            response
        )

    raise ValueError(
        "Upstreams not ready after {} seconds. Results: {}".format(
            max_seconds,
            "\n".join(
                format_response(healthcheck[0], healthcheck[1], healthcheck[2], response)
                for healthcheck, response in zip(healthchecks, responses)
            ),
        )
    )

print("All upstreams ready in {} seconds.".format(time.time() - start))
