-- track_cas.lua — versioned write of the tracker checkpoint (docs/22 §2.1, docs/11 §3.1).
-- KEYS[1] = {p}:trk:{device} (hash with fields `state` and `version`)
-- ARGV[1] = expected version (decimal; 0 = no checkpoint yet)
-- ARGV[2] = encoded checkpoint
-- ARGV[3] = TTL in milliseconds
-- Returns {1, new_version} when applied, {0, current_version} on conflict: the caller then emits
-- the sample without an estimate (docs/09 §4-bis).
local current = redis.call('HGET', KEYS[1], 'version')
if not current then
  current = '0'
end
if current ~= ARGV[1] then
  return {0, current}
end
local version = redis.call('HINCRBY', KEYS[1], 'version', 1)
redis.call('HSET', KEYS[1], 'state', ARGV[2])
redis.call('PEXPIRE', KEYS[1], ARGV[3])
return {1, tostring(version)}
