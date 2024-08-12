/// Standardizes pattern for internalised loop that allows mutable callbacks and persistent state.
/// The state is data that's persisted between iterations, and eventually passed out, this should be customisable by the caller.
pub struct Looper<State, Value> {
    pub(crate) state: State,
    pub(crate) value: Value,
    pub(crate) stop_early: bool,
}

impl<State, Value> Looper<State, Value> {
    pub(crate) fn new(state: State, value: Value) -> Self {
        Self {
            state,
            value,
            stop_early: false,
        }
    }

    /// The state of the looper.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// The state of the looper, mutable.
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    /// The value of the looper.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The value of the looper, mutable.
    pub fn value_mut(&mut self) -> &mut Value {
        &mut self.value
    }

    /// If the function using the looper supports stopping early,
    /// will cause handler to cease further iteration and return early.
    pub fn stop_early(&mut self) {
        self.stop_early = true;
    }
}
