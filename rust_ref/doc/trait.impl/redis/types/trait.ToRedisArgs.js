(function() {
    var implementors = Object.fromEntries([["bitbazaar",[["impl&lt;'a, T&gt; ToRedisArgs for <a class=\"struct\" href=\"bitbazaar/redis/struct.RedisJsonBorrowed.html\" title=\"struct bitbazaar::redis::RedisJsonBorrowed\">RedisJsonBorrowed</a>&lt;'a, T&gt;<div class=\"where\">where\n    T: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.214/serde/de/trait.Deserialize.html\" title=\"trait serde::de::Deserialize\">Deserialize</a>&lt;'a&gt;,\n    <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.reference.html\">&amp;'a T</a>: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.214/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a>,</div>"],["impl&lt;T: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.214/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> + for&lt;'a&gt; <a class=\"trait\" href=\"https://docs.rs/serde/1.0.214/serde/de/trait.Deserialize.html\" title=\"trait serde::de::Deserialize\">Deserialize</a>&lt;'a&gt;&gt; ToRedisArgs for <a class=\"struct\" href=\"bitbazaar/redis/struct.RedisJson.html\" title=\"struct bitbazaar::redis::RedisJson\">RedisJson</a>&lt;T&gt;"]]]]);
    if (window.register_implementors) {
        window.register_implementors(implementors);
    } else {
        window.pending_implementors = implementors;
    }
})()
//{"start":57,"fragment_lengths":[1150]}