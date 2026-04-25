# Public RPC Registry

`website/rpc-registry.php` is a single-file PHP endpoint for basic shared hosting.

It gives you three things:

1. `GET ?action=list`
Returns the cached list of published BlindEye RPC endpoints.

2. `GET ?action=status`
Probes the published RPC endpoints live and returns only still-working entries.

3. `POST ?action=publish`
Publishes a user's remote BlindEye RPC endpoint into the registry after probing it.

4. `POST ?action=proxy`
Acts as an HTTP bridge to a published BlindEye RPC endpoint.

## IONOS setup

Upload [website/rpc-registry.php](/mnt/c/Users/skatr/Downloads/BlindEYE/website/rpc-registry.php) to your web root or a subfolder such as:

`https://yourdomain.com/rpc-registry.php`

The file creates `rpc_registry_data.json` beside itself on first successful publish, so that directory must be writable by PHP.

## GUI wiring

In the BlindEye Mining tab:

- the app now defaults to `https://comboss.co.uk/rpc-registry.php`
- paste your hosted registry URL into `Registry URL` only if you want to override that default
- click `Refresh Public RPCs` to discover public endpoints
- start remote RPC on `0.0.0.0:18443`
- click `Publish My RPC` or leave auto-publish enabled

You can also prefill the registry URL with:

`BLINDEYE_RPC_REGISTRY_URL=https://yourdomain.com/rpc-registry.php`

## Example requests

List cached endpoints:

```bash
curl "https://yourdomain.com/rpc-registry.php?action=list"
```

Probe live endpoints:

```bash
curl "https://yourdomain.com/rpc-registry.php?action=status"
```

Publish an endpoint:

```bash
curl -X POST "https://yourdomain.com/rpc-registry.php?action=publish" \
  -H "Content-Type: application/json" \
  -d '{"rpc_url":"http://203.0.113.10:18443","owner_address":"BEC...","source":"blindeye-gui"}'
```

Bridge a JSON-RPC request over HTTP:

```bash
curl -X POST "https://yourdomain.com/rpc-registry.php?action=proxy" \
  -H "Content-Type: application/json" \
  -d '{"rpc_url":"http://203.0.113.10:18443","method":"getinfo","params":{},"id":1}'
```

## Notes

- The BlindEye node RPC is raw TCP JSON-RPC right now, not a browser-native HTTP JSON-RPC server.
- This registry file makes a website usable as a public directory and HTTP bridge without needing SSH or a VPS.
- For public use, add rate limiting or a secret publish token before opening it broadly.
