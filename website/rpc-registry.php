<?php
declare(strict_types=1);

header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Headers: Content-Type');
header('Access-Control-Allow-Methods: GET, POST, OPTIONS');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    http_response_code(204);
    exit;
}

const REGISTRY_FILE = __DIR__ . '/rpc_registry_data.json';
const PROBE_TIMEOUT_SECONDS = 3;
const MAX_ENDPOINTS = 256;
const STALE_AFTER_SECONDS = 86400;

$action = request_action();

try {
    switch ($action) {
        case 'publish':
            require_post();
            $payload = request_payload();
            $rpcUrl = trim((string)($payload['rpc_url'] ?? ''));
            $ownerAddress = trim((string)($payload['owner_address'] ?? ''));
            $source = trim((string)($payload['source'] ?? 'website'));
            $bestHeight = (int)($payload['best_height'] ?? 0);
            $connectedPeers = (int)($payload['connected_peers'] ?? 0);
            $remoteEnabled = (bool)($payload['remote_enabled'] ?? false);
            if ($rpcUrl === '') {
                json_response([
                    'ok' => false,
                    'error' => 'rpc_url is required',
                ], 400);
            }

            $verified = false;
            $lastError = '';
            try {
                $probe = probe_blindeye_rpc($rpcUrl);
                $bestHeight = (int)($probe['best_height'] ?? $bestHeight);
                $connectedPeers = (int)($probe['connected_peers'] ?? $connectedPeers);
                $remoteEnabled = (bool)($probe['rpc_allow_remote'] ?? $remoteEnabled);
                $verified = true;
            } catch (Throwable $probeError) {
                $lastError = $probeError->getMessage();
            }
            $registry = load_registry();
            $registry = upsert_registry_entry($registry, [
                'rpc_url' => canonical_rpc_url($rpcUrl),
                'owner_address' => $ownerAddress,
                'source' => $source,
                'best_height' => $bestHeight,
                'connected_peers' => $connectedPeers,
                'remote_enabled' => $remoteEnabled,
                'last_seen' => time(),
                'verified' => $verified,
                'last_error' => $lastError,
            ]);
            save_registry($registry);

            json_response([
                'ok' => true,
                'message' => $verified
                    ? 'RPC endpoint published and verified'
                    : 'RPC endpoint published as pending verification',
                'endpoint' => [
                    'rpc_url' => canonical_rpc_url($rpcUrl),
                    'owner_address' => $ownerAddress,
                    'source' => $source,
                    'best_height' => $bestHeight,
                    'connected_peers' => $connectedPeers,
                    'remote_enabled' => $remoteEnabled,
                    'last_seen' => time(),
                    'verified' => $verified,
                    'last_error' => $lastError,
                ],
            ]);
            break;

        case 'proxy':
            require_post();
            $payload = request_payload();
            $rpcUrl = trim((string)($payload['rpc_url'] ?? ''));
            $method = trim((string)($payload['method'] ?? 'getinfo'));
            $params = $payload['params'] ?? new stdClass();
            $id = (int)($payload['id'] ?? 1);
            if ($rpcUrl === '') {
                json_response([
                    'ok' => false,
                    'error' => 'rpc_url is required',
                ], 400);
            }
            $registry = load_registry();
            $knownUrls = array_column($registry, 'rpc_url');
            if (!in_array(canonical_rpc_url($rpcUrl), $knownUrls, true)) {
                json_response([
                    'ok' => false,
                    'error' => 'rpc_url must be published in the registry before proxying',
                ], 403);
            }

            $response = call_blindeye_rpc($rpcUrl, [
                'jsonrpc' => '2.0',
                'method' => $method,
                'params' => $params,
                'id' => $id,
            ]);
            json_response($response);
            break;

        case 'status':
            $registry = live_registry_status(load_registry());
            save_registry($registry);
            json_response([
                'ok' => true,
                'endpoints' => array_values($registry),
            ]);
            break;

        case 'list':
        default:
            json_response([
                'ok' => true,
                'endpoints' => array_values(load_registry()),
            ]);
            break;
    }
} catch (Throwable $error) {
    json_response([
        'ok' => false,
        'error' => $error->getMessage(),
    ], 500);
}

function request_action(): string
{
    $queryAction = $_GET['action'] ?? '';
    if (is_string($queryAction) && $queryAction !== '') {
        return strtolower($queryAction);
    }

    $payload = request_payload(false);
    $payloadAction = $payload['action'] ?? 'list';
    return strtolower((string)$payloadAction);
}

function require_post(): void
{
    if ($_SERVER['REQUEST_METHOD'] !== 'POST') {
        json_response([
            'ok' => false,
            'error' => 'This action requires POST',
        ], 405);
    }
}

function request_payload(bool $requireJson = true): array
{
    static $cached = null;
    if ($cached !== null) {
        return $cached;
    }

    $raw = file_get_contents('php://input');
    if ($raw === false || trim($raw) === '') {
        $cached = [];
        return $cached;
    }

    $decoded = json_decode($raw, true);
    if (!is_array($decoded)) {
        if ($requireJson) {
            json_response([
                'ok' => false,
                'error' => 'Request body must be JSON',
            ], 400);
        }
        $cached = [];
        return $cached;
    }

    $cached = $decoded;
    return $cached;
}

function load_registry(): array
{
    if (!file_exists(REGISTRY_FILE)) {
        return [];
    }

    $json = file_get_contents(REGISTRY_FILE);
    if ($json === false || trim($json) === '') {
        return [];
    }

    $decoded = json_decode($json, true);
    if (!is_array($decoded)) {
        return [];
    }

    $registry = [];
    foreach ($decoded as $entry) {
        if (!is_array($entry) || empty($entry['rpc_url'])) {
            continue;
        }
        $entry['rpc_url'] = canonical_rpc_url((string)$entry['rpc_url']);
        $entry['owner_address'] = (string)($entry['owner_address'] ?? '');
        $entry['source'] = (string)($entry['source'] ?? '');
        $entry['best_height'] = (int)($entry['best_height'] ?? 0);
        $entry['connected_peers'] = (int)($entry['connected_peers'] ?? 0);
        $entry['remote_enabled'] = (bool)($entry['remote_enabled'] ?? false);
        $entry['last_seen'] = (int)($entry['last_seen'] ?? 0);
        $entry['verified'] = (bool)($entry['verified'] ?? false);
        $entry['last_error'] = (string)($entry['last_error'] ?? '');
        $registry[$entry['rpc_url']] = $entry;
    }

    uasort($registry, static function (array $left, array $right): int {
        return $right['best_height'] <=> $left['best_height'];
    });

    return $registry;
}

function save_registry(array $registry): void
{
    if (count($registry) > MAX_ENDPOINTS) {
        $registry = array_slice($registry, 0, MAX_ENDPOINTS, true);
    }

    $json = json_encode(array_values($registry), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES);
    if ($json === false) {
        throw new RuntimeException('Unable to encode registry JSON');
    }

    if (file_put_contents(REGISTRY_FILE, $json, LOCK_EX) === false) {
        throw new RuntimeException('Unable to write registry file');
    }
}

function upsert_registry_entry(array $registry, array $entry): array
{
    $registry[$entry['rpc_url']] = $entry;
    uasort($registry, static function (array $left, array $right): int {
        return $right['best_height'] <=> $left['best_height'];
    });
    return $registry;
}

function live_registry_status(array $registry): array
{
    $now = time();
    foreach ($registry as $rpcUrl => $entry) {
        try {
            $probe = probe_blindeye_rpc($rpcUrl);
            $entry['best_height'] = (int)($probe['best_height'] ?? 0);
            $entry['connected_peers'] = (int)($probe['connected_peers'] ?? 0);
            $entry['remote_enabled'] = (bool)($probe['rpc_allow_remote'] ?? false);
            $entry['last_seen'] = $now;
            $entry['verified'] = true;
            $entry['last_error'] = '';
            $registry[$rpcUrl] = $entry;
        } catch (Throwable $error) {
            if (($entry['last_seen'] ?? 0) + STALE_AFTER_SECONDS < $now) {
                unset($registry[$rpcUrl]);
            } else {
                $entry['verified'] = false;
                $entry['last_error'] = $error->getMessage();
                $registry[$rpcUrl] = $entry;
            }
        }
    }

    uasort($registry, static function (array $left, array $right): int {
        return $right['best_height'] <=> $left['best_height'];
    });

    return $registry;
}

function probe_blindeye_rpc(string $rpcUrl): array
{
    $response = call_blindeye_rpc($rpcUrl, [
        'jsonrpc' => '2.0',
        'method' => 'getinfo',
        'params' => new stdClass(),
        'id' => 1,
    ]);

    if (!is_array($response)) {
        throw new RuntimeException('RPC returned an invalid response');
    }
    if (!empty($response['error'])) {
        $error = is_string($response['error']) ? $response['error'] : 'unknown RPC error';
        throw new RuntimeException('RPC error: ' . $error);
    }
    $result = $response['result'] ?? null;
    if (!is_array($result)) {
        throw new RuntimeException('RPC did not return a result object');
    }

    return $result;
}

function call_blindeye_rpc(string $rpcUrl, array $request): array
{
    [$host, $port] = parse_rpc_host_port($rpcUrl);
    $socket = @stream_socket_client(
        sprintf('tcp://%s:%d', $host, $port),
        $errorCode,
        $errorMessage,
        PROBE_TIMEOUT_SECONDS
    );
    if (!$socket) {
        throw new RuntimeException(sprintf('Unable to reach %s: %s', canonical_rpc_url($rpcUrl), $errorMessage));
    }

    stream_set_timeout($socket, PROBE_TIMEOUT_SECONDS);
    $payload = json_encode($request, JSON_UNESCAPED_SLASHES);
    if ($payload === false) {
        fclose($socket);
        throw new RuntimeException('Unable to encode RPC request');
    }

    fwrite($socket, $payload . "\n");
    $line = fgets($socket);
    fclose($socket);
    if ($line === false) {
        throw new RuntimeException('RPC endpoint closed the connection without a response');
    }

    $decoded = json_decode(trim($line), true);
    if (!is_array($decoded)) {
        throw new RuntimeException('RPC endpoint returned invalid JSON');
    }

    return $decoded;
}

function parse_rpc_host_port(string $rpcUrl): array
{
    $canonical = canonical_rpc_url($rpcUrl);
    $parts = parse_url($canonical);
    if (!is_array($parts) || empty($parts['host']) || empty($parts['port'])) {
        throw new RuntimeException('rpc_url must look like host:port or http://host:port');
    }

    return [(string)$parts['host'], (int)$parts['port']];
}

function canonical_rpc_url(string $rpcUrl): string
{
    $trimmed = trim($rpcUrl);
    if ($trimmed === '') {
        return '';
    }
    if (strpos($trimmed, '://') === false) {
        return 'tcp://' . $trimmed;
    }

    $parts = parse_url($trimmed);
    if (!is_array($parts) || empty($parts['host']) || empty($parts['port'])) {
        throw new RuntimeException('rpc_url must include a host and port');
    }

    return sprintf(
        '%s://%s:%d',
        $parts['scheme'] ?? 'tcp',
        $parts['host'],
        (int)$parts['port']
    );
}

function json_response(array $payload, int $status = 200): void
{
    http_response_code($status);
    echo json_encode($payload, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES);
    exit;
}
