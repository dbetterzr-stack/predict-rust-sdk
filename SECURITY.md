# Security policy

Do not open a public Issue containing credentials, account identifiers, signed
authentication payloads, or live order data. Report security issues privately
to the repository owner.

The SDK accepts secrets from its caller and must never persist or log them.
Only synthetic keys and identifiers may appear in tests and examples.
