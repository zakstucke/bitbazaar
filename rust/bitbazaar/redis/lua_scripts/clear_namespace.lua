local cursor="0";
local count = 0;
repeat
 local scanResult = redis.call("SCAN", cursor, "MATCH", ARGV[1]..'*', "COUNT", 100);
	local keys = scanResult[2];
	for i = 1, #keys do
		local key = keys[i];
		redis.replicate_commands()
		redis.call("DEL", key);
		count = count +1;
	end;
	cursor = scanResult[1];
until cursor == "0";
