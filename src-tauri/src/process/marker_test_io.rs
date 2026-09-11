use std::{cell::RefCell, marker::PhantomData, ops::Range, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Operation {
    Read,
    Write,
    Written,
}
struct State {
    range: Range<usize>,
    fault: Option<(usize, Operation)>,
    action: Option<Action>,
    writes: usize,
}
struct Action {
    address: usize,
    operation: Operation,
    remaining: usize,
    run: Box<dyn FnOnce()>,
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
pub struct Scope(Option<State>, PhantomData<Rc<()>>);
impl Scope {
    pub fn new(range: Range<usize>) -> Self {
        Self(
            STATE.replace(Some(State {
                range,
                fault: None,
                action: None,
                writes: 0,
            })),
            PhantomData,
        )
    }
    pub fn fail(&self, address: usize, operation: Operation) {
        fail_next(address, operation);
    }
    pub fn writes(&self) -> usize {
        STATE.with_borrow(|state| state.as_ref().unwrap().writes)
    }
    pub fn on_nth(&self, trigger: (usize, Operation, usize), run: impl FnOnce() + 'static) {
        let (address, operation, remaining) = trigger;
        assert!(remaining > 0);
        STATE.with_borrow_mut(|state| {
            let state = state.as_mut().unwrap();
            assert!(state.range.contains(&address));
            state.action = Some(Action {
                address,
                operation,
                remaining,
                run: Box::new(run),
            });
        });
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        STATE.set(self.0.take());
    }
}
pub fn intercept(address: usize, operation: Operation) -> Result<(), String> {
    let action = STATE.with_borrow_mut(|state| {
        let state = state.as_mut()?;
        let action = state.action.as_mut()?;
        if action.address != address || action.operation != operation {
            return None;
        }
        action.remaining -= 1;
        if action.remaining == 0 {
            state.action.take()
        } else {
            None
        }
    });
    if let Some(action) = action {
        (action.run)();
    }
    STATE.with_borrow_mut(|state| {
        let Some(state) = state
            .as_mut()
            .filter(|state| state.range.contains(&address))
        else {
            return Ok(());
        };
        if operation == Operation::Write {
            state.writes += 1;
        }
        if state.fault == Some((address, operation)) {
            state.fault = None;
            return Err(format!("injected {operation:?} failure at {address:#x}"));
        }
        Ok(())
    })
}
pub fn fail_next(address: usize, operation: Operation) {
    STATE.with_borrow_mut(|state| state.as_mut().unwrap().fault = Some((address, operation)));
}
