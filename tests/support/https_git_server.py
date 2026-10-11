"""A git server over HTTPS for the tests, like GitHub with a personal access token.

Usage: python3 https_git_server.py <folder>

<folder>/repositories  the bare repositories (GIT_PROJECT_ROOT of git http-backend)
<folder>/token         the token that the server accepts; read again for each request
<folder>/cert.pem      made by this script (self-signed, for 127.0.0.1); give it to git as GIT_SSL_CAINFO
<folder>/port          written by this script when the server is ready

Any user name is accepted. The password must be the token, as on GitHub.
"""

import base64
import http.server
import os
import ssl
import subprocess
import sys

folder = sys.argv[1]


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def authorized(self):
        with open(os.path.join(folder, "token")) as f:
            token = f.read().strip()
        header = self.headers.get("Authorization", "")
        if not header.startswith("Basic "):
            return False
        _, _, password = base64.b64decode(header[6:]).decode().partition(":")
        return password == token

    def handle_git(self):
        if not self.authorized():
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="test"')
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        path, _, query = self.path.partition("?")
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        env = {
            "PATH": os.environ["PATH"],
            "GIT_PROJECT_ROOT": os.path.join(folder, "repositories"),
            "GIT_HTTP_EXPORT_ALL": "1",
            "REMOTE_USER": "printer",
            "REQUEST_METHOD": self.command,
            "PATH_INFO": path,
            "QUERY_STRING": query,
            "CONTENT_TYPE": self.headers.get("Content-Type", ""),
            "CONTENT_LENGTH": str(len(body)),
            "HTTP_CONTENT_ENCODING": self.headers.get("Content-Encoding", ""),
            "GIT_PROTOCOL": self.headers.get("Git-Protocol", ""),
        }
        result = subprocess.run(
            ["git", "http-backend"], input=body, env=env, capture_output=True
        )
        head, _, payload = result.stdout.partition(b"\r\n\r\n")
        status = 200
        headers = []
        for line in head.decode().split("\r\n"):
            name, _, value = line.partition(": ")
            if name.lower() == "status":
                status = int(value.split()[0])
            elif name:
                headers.append((name, value))
        self.send_response(status)
        for name, value in headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    do_GET = handle_git
    do_POST = handle_git


cert = os.path.join(folder, "cert.pem")
key = os.path.join(folder, "key.pem")
subprocess.run(
    ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
     "-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1",
     "-keyout", key, "-out", cert],
    check=True, capture_output=True,
)
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(cert, key)
server.socket = context.wrap_socket(server.socket, server_side=True)
with open(os.path.join(folder, "port.new"), "w") as f:
    f.write(str(server.server_address[1]))
os.rename(os.path.join(folder, "port.new"), os.path.join(folder, "port"))
server.serve_forever()
