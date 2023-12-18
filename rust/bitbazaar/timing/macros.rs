#[macro_export]
macro_rules! timeit {
    ($desc:expr, $code:block) => {{
        use $crate::timing::GLOBAL_TIME_RECORDER;

        let _res = GLOBAL_TIME_RECORDER.timeit($desc, || $code);

        _res
    }};
}
