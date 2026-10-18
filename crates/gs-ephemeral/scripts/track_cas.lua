-- track_cas.lua — atomic versioned compare-and-set of a tracker checkpoint.
--
-- Reference implementation for the Redis/Valkey backend of `EphemeralStore`
-- (`docs/11` §3, `docs/22` §2.1, T-E2-06). The in-memory backend implements the
-- same semantics natively (`compare_and_set_checkpoint`).
--
-- The HTTP ingest has no instance affinity, so two instances can read-modify-
-- write the same device's checkpoint. Without the version check one of them
-- loses its update silently; this script makes the loser retry.
--
-- KEYS[1] = "{p}:trk:{device}"
-- ARGV[1] = expected_version (decimal; 0 means "no checkpoint yet")
-- ARGV[2] = new value (the encoded checkpoint)
-- ARGV[3] = ttl seconds
--
-- Returns { status, version }:
--   { 1, new_version }  -> applied
--   { 0, current }      -> conflict; the caller re-reads and retries once
--
-- Note: Redis hashes cannot hold binary blobs as values, so the value and the
-- version live in a single string as "<version>\n<blob>". The version is parsed
-- out; the blob is opaque to Redis.

local key = KEYS[1]
local expected = tonumber(ARGV[1])
local value = ARGV[2]
local ttl = tonumber(ARGV[3])

local raw = redis.call('GET', key)
local current = 0
if raw then
  local sep = string.find(raw, '\n', 1, true)
  if sep then
    current = tonumber(string.sub(raw, 1, sep - 1)) or 0
  end
end

if current ~= expected then
  return { 0, current }
end

local next_version = current + 1
redis.call('SET', key, tostring(next_version) .. '\n' .. value, 'EX', ttl)
return { 1, next_version }
