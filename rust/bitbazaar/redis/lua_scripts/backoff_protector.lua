local start_delaying_after_attempt = tonumber(ARGV[1])
local initial_delay_ms = tonumber(ARGV[2])
local multiplier = tonumber(ARGV[3])
local key = KEYS[1]

-- Attempt to get the value of the key
local value = redis.call("GET", key)
local past_attempts
local last_attempt_at_utc_ms
if value then
    local tuple = cjson.decode(value)
    past_attempts = tuple[1]
    last_attempt_at_utc_ms = tuple[2]
else
    past_attempts = 0
    last_attempt_at_utc_ms = nil
end

local active_delay_ms
local next_delay_ms
if past_attempts >= start_delaying_after_attempt then
    local applicable_attempts = past_attempts - start_delaying_after_attempt
    active_delay_ms = initial_delay_ms * math.pow(multiplier, applicable_attempts)
    next_delay_ms = initial_delay_ms * math.pow(multiplier, applicable_attempts + 1)
else
    active_delay_ms = 0
    next_delay_ms = initial_delay_ms
end

local now_utc_ms = redis.call("TIME")[1] * 1000 + redis.call("TIME")[2] / 1000
if last_attempt_at_utc_ms then
    if now_utc_ms - last_attempt_at_utc_ms < active_delay_ms then
        return active_delay_ms - (now_utc_ms - last_attempt_at_utc_ms)
    else
        -- Expiry delay * 2 as this is when the attempt count resets completely.
        redis.call("SET", key, cjson.encode({past_attempts + 1, now_utc_ms}), "PX", next_delay_ms * 2)
        return 0
    end
else -- First attempt
    -- Expiry delay * 2 as this is when the attempt count resets completely.
    redis.call("SET", key, cjson.encode({1, now_utc_ms}), "PX", next_delay_ms * 2)
    return 0
end