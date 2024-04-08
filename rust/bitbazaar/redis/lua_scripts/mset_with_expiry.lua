-- Expiry in milliseconds is passed as the first argument, therefore KEYS[i] = ARGV[i + 1]
local expiry = tonumber(ARGV[1])

for i = 1, #KEYS do
    local key = KEYS[i]
    local value = ARGV[i + 1]
    redis.call("SET", key, value, "PX", expiry)
end
