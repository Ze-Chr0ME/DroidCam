import datetime
import hmac
import ipaddress
import json
import logging
from pathlib import Path
import secrets
import socket
import ssl
import threading
import time

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID

from .protocol import normalize_frame, read_frame, read_json, send_json, STATUS_MESSAGE
from .decoder import Decoder
from .settings import Settings, needs_reconnect

PORT = 47950


def pairing_token(directory):
    """Keep trusted-device pairing across PC restarts, alongside its pinned certificate."""
    path = Path(directory) / "pairing-token.txt"
    try:
        token = path.read_text(encoding="ascii").strip()
        if len(token) != 64 or any(c not in "0123456789abcdef" for c in token):
            raise ValueError("Stored pairing token is invalid; remove pairing-token.txt and pair again")
        return token
    except FileNotFoundError:
        token = secrets.token_hex(32)
        try:
            with path.open("x", encoding="ascii") as output:
                output.write(token + "\n")
        except FileExistsError:
            return pairing_token(directory)
        return token


def make_identity(directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    key_path, cert_path = directory / "key.pem", directory / "cert.pem"
    if not key_path.exists() or not cert_path.exists():
        key = ec.generate_private_key(ec.SECP256R1())
        name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "CameraBridge local receiver")])
        now = datetime.datetime.now(datetime.timezone.utc)
        cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
                .public_key(key.public_key()).serial_number(x509.random_serial_number())
                .not_valid_before(now - datetime.timedelta(minutes=5))
                .not_valid_after(now + datetime.timedelta(days=3650))
                .sign(key, hashes.SHA256()))
        key_path.write_bytes(key.private_bytes(serialization.Encoding.PEM,
                             serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        cert_path.write_bytes(cert.public_bytes(serialization.Encoding.PEM))
    cert = x509.load_pem_x509_certificate(cert_path.read_bytes())
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(cert_path, key_path)
    return context, cert.fingerprint(hashes.SHA256()).hex()


def local_addresses():
    addresses = set()
    try:
        for item in socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET):
            addr = item[4][0]
            if not ipaddress.ip_address(addr).is_loopback:
                addresses.add(addr)
    except OSError:
        pass
    return sorted(addresses) or ["127.0.0.1"]


class Receiver:
    def __init__(self, identity_dir, status, frame, port=PORT, host="0.0.0.0"):
        self.context, self.fingerprint = make_identity(identity_dir)
        self.token = pairing_token(identity_dir)
        self.status, self.frame = status, frame
        self.port, self.host = port, host
        self.stop_event = threading.Event()
        self.listener = self.client = None
        self.thread = None
        self.connected = False
        self.mirror = False
        self.settings = Settings()
        self.desired_stream = True
        self.revision = 0
        self.remote_control = False

    def configure(self, settings, activate=False):
        reconnect = needs_reconnect(self.settings, settings)
        if activate and not self.desired_stream:
            reconnect = True
        if activate:
            self.desired_stream = True
        self.settings = settings
        self.mirror = settings.mirror
        if reconnect:
            self.revision += 1
            self._reconnect()

    def set_streaming(self, enabled):
        if self.desired_stream != enabled:
            self.desired_stream = enabled
            self.revision += 1
            self._reconnect()

    def _reconnect(self):
        if self.client:
            try:
                self.client.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass

    def pairing(self, host):
        return json.dumps(dict(v=1, host=host, port=self.port, token=self.token,
                               fingerprint=self.fingerprint), separators=(",", ":"))

    def start(self):
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        self.listener.bind((self.host, self.port))
        self.port = self.listener.getsockname()[1]
        self.listener.listen(4)
        self.listener.settimeout(0.5)
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while not self.stop_event.is_set():
            try:
                raw, address = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            try:
                raw.settimeout(5)
                raw.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                with self.context.wrap_socket(raw, server_side=True) as stream:
                    self.client = stream
                    hello = read_json(stream)
                    token = hello.get("token")
                    if hello.get("v") not in (1, 2) or hello.get("codec") != "h264" or not isinstance(token, str) or not hmac.compare_digest(token, self.token):
                        send_json(stream, {"ok": False, "error": "Pair again using the PC QR code"})
                        continue
                    settings = self.settings
                    revision = self.revision
                    self.remote_control = hello.get("v") == 2 and hello.get("remote_control") is True
                    decoder = Decoder(hardware=settings.decoder == "auto")
                    send_json(stream, {"ok": True, "codec": "h264", "revision": revision,
                                       "remote_control": self.remote_control,
                                       "streaming": self.desired_stream, **settings.wire()})
                    self.connected = True
                    if hello.get("v") == 1 and hello.get("stream_settings") is not True:
                        self.status("notice", "Older phone app: install the new APK to use stream settings")
                    self.status("connected", "Phone connected · PC control ready" if self.remote_control else "Phone connected · update APK for remote control")
                    count, start = 0, time.monotonic()
                    decode_seconds = 0.0
                    while not self.stop_event.is_set():
                        payload, rotation = read_frame(stream, allow_status=self.remote_control)
                        if not payload:
                            continue
                        if rotation == STATUS_MESSAGE:
                            state = json.loads(payload)
                            if not isinstance(state, dict):
                                raise ValueError("Invalid phone state")
                            if type(state.get("revision")) is int and state["revision"] == self.revision:
                                self.status("phone_state", state)
                            continue
                        decode_start = time.monotonic()
                        for decoded in decoder.decode(payload):
                            self.frame(normalize_frame(decoded, rotation, size=settings.size, mirror=self.mirror))
                            count += 1
                        decode_seconds += time.monotonic() - decode_start
                        elapsed = time.monotonic() - start
                        if elapsed >= 1:
                            average_ms = decode_seconds * 1000 / max(count, 1)
                            self.status("fps", f"Received {count / elapsed:.0f} fps · {settings.resolution} · decode {average_ms:.1f} ms/frame")
                            logging.info("Video %.1f fps, %s, decode %.1f ms/frame, decoder %s",
                                         count / elapsed, settings.resolution, average_ms,
                                         "D3D11 context" if decoder.hardware else "CPU")
                            count, start = 0, time.monotonic()
                            decode_seconds = 0.0
            except (OSError, EOFError, ValueError) as error:
                if self.connected:
                    self.status("disconnected", "Connection lost · " + str(error))
            except Exception as error:
                logging.exception("Video receiver failed")
                self.status("error", "Video error · " + str(error))
            finally:
                raw.close()
                was_connected = self.connected
                self.client = None
                self.connected = False
                self.remote_control = False
                if was_connected:
                    self.status("disconnected", "Waiting for your phone to reconnect")

    def stop(self):
        self.stop_event.set()
        if self.client:
            try:
                self.client.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        if self.listener:
            self.listener.close()
        if self.thread:
            self.thread.join(timeout=6)
