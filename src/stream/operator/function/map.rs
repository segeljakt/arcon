// Copyright (c) 2020, KTH Royal Institute of Technology.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::{
    data::{ArconElement, ArconNever, ArconType},
    index::ArconState,
    stream::operator::{Operator, OperatorContext},
    util::ArconFnBounds,
};
use arcon_error::*;
use arcon_state::Backend;
use kompact::prelude::ComponentDefinition;
use std::marker::PhantomData;

pub struct Map<IN, OUT, F, S>
where
    IN: ArconType,
    OUT: ArconType,
    F: Fn(IN, &mut S) -> OperatorResult<OUT> + ArconFnBounds,
    S: ArconState,
{
    state: S,
    udf: F,
    _marker: PhantomData<fn(IN) -> OperatorResult<OUT>>,
}

impl<IN, OUT> Map<IN, OUT, fn(IN, &mut ()) -> OperatorResult<OUT>, ()>
where
    IN: ArconType,
    OUT: ArconType,
{
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        udf: impl Fn(IN) -> OUT + ArconFnBounds,
    ) -> Map<IN, OUT, impl Fn(IN, &mut ()) -> OperatorResult<OUT> + ArconFnBounds, ()> {
        let udf = move |input: IN, _: &mut ()| {
            let output = udf(input);
            Ok(output)
        };

        Map {
            state: (),
            udf,
            _marker: Default::default(),
        }
    }
}

impl<IN, OUT, F, S> Map<IN, OUT, F, S>
where
    IN: ArconType,
    OUT: ArconType,
    F: Fn(IN, &mut S) -> OperatorResult<OUT> + ArconFnBounds,
    S: ArconState,
{
    pub fn stateful(state: S, udf: F) -> Self {
        Map {
            state,
            udf,
            _marker: Default::default(),
        }
    }
}

impl<IN, OUT, F, S> Operator for Map<IN, OUT, F, S>
where
    IN: ArconType,
    OUT: ArconType,
    F: Fn(IN, &mut S) -> OperatorResult<OUT> + ArconFnBounds,
    S: ArconState,
{
    type IN = IN;
    type OUT = OUT;
    type TimerState = ArconNever;
    type OperatorState = S;

    fn handle_element(
        &mut self,
        element: ArconElement<IN>,
        mut ctx: OperatorContext<Self, impl Backend, impl ComponentDefinition>,
    ) -> OperatorResult<()> {
        ctx.output(ArconElement {
            data: (self.udf)(element.data, &mut self.state)?,
            timestamp: element.timestamp,
        });

        Ok(())
    }

    crate::ignore_timeout!();

    fn persist(&mut self) -> Result<(), arcon_state::error::ArconStateError> {
        self.state.persist()
    }
    fn state(&mut self) -> &mut Self::OperatorState {
        &mut self.state
    }
}
