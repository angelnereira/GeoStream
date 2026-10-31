-- seq_cas.lua — compare-and-set of the last accepted sequence (docs/09 §4, ADR-0008, docs/11 §3).
-- KEYS[1] = {p}:seq:{session}:{device}
-- ARGV[1] = current (the value assumed when the key is absent)
-- ARGV[2] = next
-- ARGV[3] = TTL in milliseconds, renewed on every call
-- Returns {outcome, stored}: outcome is "accepted", "duplicate" or "stale"; stored is the value
-- before the call. Sequences are u64 and compared as decimal strings: Lua numbers are doubles and
-- would lose precision above 2^53.
local function is_decimal(s)
  return type(s) == 'string' and string.match(s, '^%d+$') ~= nil
end
local function strip(s)
  local t = string.gsub(s, '^0+', '')
  if t == '' then return '0' end
  return t
end
local function less(a, b)
  if #a ~= #b then return #a < #b end
  return a < b
end

local current = ARGV[1]
local next_value = ARGV[2]
if not is_decimal(current) or not is_decimal(next_value) then
  return redis.error_reply('ERR seq_cas arguments are not decimal')
end
current = strip(current)
next_value = strip(next_value)

local stored = redis.call('GET', KEYS[1])
if not stored then
  stored = current
elseif not is_decimal(stored) then
  return redis.error_reply('ERR seq_cas stored value is not decimal')
end
stored = strip(stored)

local outcome
local written = stored
if less(stored, next_value) and not less(stored, current) then
  outcome = 'accepted'
  written = next_value
elseif stored == next_value then
  outcome = 'duplicate'
else
  outcome = 'stale'
end
redis.call('SET', KEYS[1], written, 'PX', ARGV[3])
return {outcome, stored}
