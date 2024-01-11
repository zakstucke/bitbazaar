local results = {}
for i, key in ipairs(KEYS) do
    results[i] = redis.call('EXISTS', key) == 1
end
return results
