-- devseq_max.lua — SET only if greater (docs/11 §3, `{p}:devseq:{device}`).
-- KEYS[1] = counter key
-- ARGV[1] = candidate value (u64, decimal)
-- ARGV[2] = TTL in milliseconds, renewed on every call
-- Returns the effective value as a decimal string. An absent key counts as 0.
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

local candidate = ARGV[1]
if not is_decimal(candidate) then
  return redis.error_reply('ERR devseq_max argument is not decimal')
end
candidate = strip(candidate)

local stored = redis.call('GET', KEYS[1])
if not stored then
  stored = '0'
elseif not is_decimal(stored) then
  return redis.error_reply('ERR devseq_max stored value is not decimal')
end
stored = strip(stored)

local effective = stored
if less(stored, candidate) then
  effective = candidate
end
redis.call('SET', KEYS[1], effective, 'PX', ARGV[2])
return effective
