These are disposable, public test credentials for the loopback HTTPS server.
The self-signed certificate deliberately names `wrong-host.invalid`, so tests
exercise both untrusted certificates and hostname mismatch with `tls_insecure`.
Never use this key on a real device or service.
