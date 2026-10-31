-- presence_release.lua — delete presence only if `connection_id` matches (docs/04 §6.3, docs/11 §3).
-- KEYS[1] = {p}:presence:{device}
-- ARGV[1] = connection_id of the closing connection
-- Returns 1 when the key was deleted, 0 otherwise.
if redis.call('HGET', KEYS[1], 'connection_id') == ARGV[1] then
  redis.call('DEL', KEYS[1])
  return 1
end
return 0
