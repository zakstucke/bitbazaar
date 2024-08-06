use std::sync::atomic::AtomicI64;

use chrono::TimeDelta;

/// State passed into the callback of [`PeriodicUpdater`].
pub struct PeriodicUpdaterState<'a, Ctx> {
    ctx: &'a Ctx,
    elapsed: TimeDelta,
}

impl<'a, Ctx> PeriodicUpdaterState<'a, Ctx> {
    /// Get the context, only useful if [`PeriodicUpdater::new_with_ctx`] was used.
    pub fn ctx(&self) -> &'a Ctx {
        self.ctx
    }

    /// Get the elapsed time since the last call.
    pub fn elapsed(&self) -> TimeDelta {
        self.elapsed
    }
}

macro_rules! gen {
    ([$($impl_generics:tt)*], $struct_name:ident) => {
        /// A periodic updater. Run a callback at a specified time interval. Synchronous. Requires polling.
        /// Useful in long running loops to do something at a specified time interval. [`PeriodicUpdater::maybe_update`] should be called during each loop.
        pub struct $struct_name<'a, A, Ctx = ()> {
            last_timestamp_ms: AtomicI64,
            update_every: TimeDelta,
            on_maybe_update: Option<Box<dyn Fn(PeriodicUpdaterState<Ctx>, A) + $($impl_generics)*>>,
            on_update: Box<dyn Fn(PeriodicUpdaterState<Ctx>, A) + $($impl_generics)*>,
            ctx: Ctx,
            _a: std::marker::PhantomData<A>,
        }

        impl<'a, A: Clone> $struct_name<'a, A, ()> {
            /// Create a new periodic update that runs a callback on each update.
            /// Make sure to call [`Self::maybe_update`] frequently.
            ///
            /// Arguments:
            /// - `update_every`: The time interval at which to run the callback.
            /// - `on_update`: The callback to run. Is passed the state and custom params.
            pub fn new(
                update_every: TimeDelta,
                on_update: impl Fn(PeriodicUpdaterState<()>, A) + $($impl_generics)*,
            ) -> Self {
                Self {
                    on_maybe_update: None,
                    on_update: Box::new(on_update),
                    last_timestamp_ms: AtomicI64::new(0),
                    update_every,
                    ctx: (),
                    _a: std::marker::PhantomData,
                }
            }

            /// Create a new periodic updater that also runs a callback each time [`Self::maybe_update`] is called.
            /// Make sure to call [`Self::maybe_update`] frequently.
            ///
            /// Arguments:
            /// - `update_every`: The time interval at which to run the callback.
            /// - `on_maybe_update`: The callback to run each time [`PeriodicUpdater::maybe_update`] is called. Is passed the state and custom params.
            /// - `on_update`: The callback to run. Is passed the state and custom params.
            pub fn new_with_on_maybe_update(
                update_every: TimeDelta,
                on_maybe_update: impl Fn(PeriodicUpdaterState<()>, A) + $($impl_generics)*,
                on_update: impl Fn(PeriodicUpdaterState<()>, A) + $($impl_generics)*,
            ) -> Self {
                Self {
                    on_maybe_update: Some(Box::new(on_maybe_update)),
                    on_update: Box::new(on_update),
                    last_timestamp_ms: AtomicI64::new(0),
                    update_every,
                    ctx: (),
                    _a: std::marker::PhantomData,
                }
            }
        }

        impl<'a, A: Clone, Ctx> $struct_name<'a, A, Ctx> {
            /// Create a new periodic updater with custom context.
            /// Make sure to call [`Self::maybe_update`] frequently.
            ///
            /// Arguments:
            /// - `ctx`: The context to pass to the callback.
            /// - `update_every`: The time interval at which to run the callback.
            /// - `on_update`: The callback to run. Is passed the state and custom params.
            pub fn new_with_ctx(
                ctx: Ctx,
                update_every: TimeDelta,
                on_update: impl Fn(PeriodicUpdaterState<Ctx>, A) + $($impl_generics)*,
            ) -> Self {
                Self {
                    on_maybe_update: None,
                    on_update: Box::new(on_update),
                    last_timestamp_ms: AtomicI64::new(0),
                    update_every,
                    ctx,
                    _a: std::marker::PhantomData,
                }
            }

            /// Create a new periodic updater with custom context.
            /// Make sure to call [`Self::maybe_update`] frequently.
            ///
            /// Arguments:
            /// - `ctx`: The context to pass to the callback.
            /// - `update_every`: The time interval at which to run the callback.
            /// - `on_maybe_update`: The callback to run each time [`Self::maybe_update`] is called. Is passed the state and custom params.
            /// - `on_update`: The callback to run. Is passed the state and custom params.
            pub fn new_with_ctx_and_on_maybe_update(
                ctx: Ctx,
                update_every: TimeDelta,
                on_maybe_update: impl Fn(PeriodicUpdaterState<Ctx>, A) + $($impl_generics)*,
                on_update: impl Fn(PeriodicUpdaterState<Ctx>, A) + $($impl_generics)*,
            ) -> Self {
                Self {
                    on_maybe_update: Some(Box::new(on_maybe_update)),
                    on_update: Box::new(on_update),
                    last_timestamp_ms: AtomicI64::new(0),
                    update_every,
                    ctx,
                    _a: std::marker::PhantomData,
                }
            }

            /// Get the context, only useful if [`Self::new_with_ctx`] was used.
            pub fn ctx(&self) -> &Ctx {
                &self.ctx
            }

            /// Call this function frequently to check if the callback should be run.
            pub fn maybe_update(&self, ext_params: A) {
                let epoch_ms = chrono::Utc::now().timestamp_millis();
                let elapsed = TimeDelta::milliseconds(
                    epoch_ms
                        - self
                            .last_timestamp_ms
                            .load(std::sync::atomic::Ordering::Relaxed),
                );

                let call_update = elapsed >= self.update_every;

                // Just doing this to prevent cloning unless both on_maybe_update and on_update need calling:
                let mut maybe_clone_params = Some(ext_params);

                if let Some(on_maybe_update) = &self.on_maybe_update {
                    (on_maybe_update)(
                        PeriodicUpdaterState {
                            elapsed,
                            ctx: self.ctx(),
                        },
                        if call_update {
                            maybe_clone_params.clone().unwrap()
                        } else {
                            maybe_clone_params.take().unwrap()
                        },
                    );
                }

                if call_update {
                    (self.on_update)(
                        PeriodicUpdaterState {
                            elapsed,
                            ctx: self.ctx(),
                        },
                        maybe_clone_params.unwrap(),
                    );
                    self.last_timestamp_ms
                        .store(epoch_ms, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
    };
}

// A cross thread version, plus a thread local version.
gen!([Send + Sync + 'a], PeriodicUpdater);
gen!(['a], LocalPeriodicUpdater);
