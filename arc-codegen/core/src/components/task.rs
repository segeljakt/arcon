use crate::components::task_manager::Metric;
use crate::components::task_manager::MetricPort;
use messages::protobuf::*;

use crate::destination::Destination;
use crate::error::Error;
use crate::error::ErrorKind::*;
use crate::weld::*;
use state_backend::StateBackend;
use kompact::*;
use std::sync::Arc;
use std::time::Duration;
use weld::*;

#[derive(ComponentDefinition)]
pub struct Task {
    ctx: ComponentContext<Task>,
    report_timer: Option<ScheduledTimer>,
    pub manager_port: RequiredPort<MetricPort, Task>,
    destination: Option<Destination>,
    udf: Module,
    udf_avg: u64,
    udf_executions: u64,
    backend: Arc<StateBackend>,
    id: String,
}

impl Task {
    pub fn new(
        id: String,
        udf: Module,
        backend: Arc<StateBackend>,
        destination: Option<Destination>,
    ) -> Task {
        Task {
            ctx: ComponentContext::new(),
            report_timer: None,
            manager_port: RequiredPort::new(),
            destination,
            udf,
            udf_avg: 0,
            udf_executions: 0,
            backend: Arc::clone(&backend),
            id,
        }
    }

    fn stop_report(&mut self) {
        if let Some(timer) = self.report_timer.clone() {
            self.cancel_timer(timer);
            self.report_timer = None;
        }
    }

    fn run_udf(&mut self, e: Element) -> crate::error::Result<()> {
        if e.get_task_id() != self.id {
            let err_fmt = format!(
                "given task id {} does not match {}",
                e.get_task_id(),
                self.id
            );
            Err(Error::new(BadTaskError(err_fmt)))
        } else {
            let raw = e.get_data();
            let s = u8_slice_to_i8(raw);
            let input: WeldVec<i8> = WeldVec::new(s.as_ref().as_ptr(), s.as_ref().len() as i64);

            let ref mut ctx = WeldContext::new(&self.udf.conf()).map_err(|e| {
                Error::new(ContextError(e.message().to_string_lossy().into_owned()))
            })?;
            let run: ModuleRun<WeldVec<i8>> = self.udf.run(&input, ctx)?;
            let ns = run.1;
            self.update_avg(ns);

            if let Some(dest) = &self.destination {
                let mut msg = TaskMsg::new();
                let mut element = Element::new();
                element.set_timestamp(crate::util::get_system_time());
                element.set_id(e.get_id());
                element.set_task_id(dest.task_id.clone());
                let to_raw = weld_to_raw(run.0).unwrap();
                element.set_data(i8_slice_to_u8(&to_raw).to_vec());
                msg.set_element(element);

                dest.path.tell(msg, self);
            }

            Ok(())
        }
    }

    fn update_avg(&mut self, ns: u64) {
        if self.udf_executions == 0 {
            self.udf_avg = ns;
        } else {
            let ema: i32 = ((ns as f32 - self.udf_avg as f32)
                * (2.0 / (self.udf_executions + 1) as f32)) as i32
                + self.udf_avg as i32;
            self.udf_avg = ema as u64;
        }
        self.udf_executions += 1;
    }
}

impl Provide<ControlPort> for Task {
    fn handle(&mut self, event: ControlEvent) -> () {
        match event {
            ControlEvent::Start => {
                info!(self.ctx.log(), "Task {} Starting up", self.id);
                let timeout = Duration::from_millis(250);
                let timer = self.schedule_periodic(timeout, timeout, |self_c, _| {
                    self_c.manager_port.trigger(Metric {
                        task_id: self_c.id.clone(),
                        task_avg: self_c.udf_avg,
                    });
                });

                self.report_timer = Some(timer);
            }
            ControlEvent::Stop => self.stop_report(),
            ControlEvent::Kill => self.stop_report(),
        }
    }
}

impl Actor for Task {
    fn receive_local(&mut self, _sender: ActorRef, _msg: Box<Any>) {}
    fn receive_message(&mut self, sender: ActorPath, ser_id: u64, buf: &mut Buf) {
        if ser_id == serialisation_ids::PBUF {
            let r: Result<TaskMsg, SerError> = ProtoSer::deserialise(buf);
            if let Ok(msg) = r {
                match msg.payload.unwrap() {
                    TaskMsg_oneof_payload::watermark(_) => {}
                    TaskMsg_oneof_payload::element(e) => {
                        if let Err(err) = self.run_udf(e) {
                            error!(
                                self.ctx.log(),
                                "Failed to run Task UDF with err: {}",
                                err.to_string()
                            );
                        }
                    }
                    TaskMsg_oneof_payload::checkpoint(_) => {
                        let _ = self.backend.checkpoint("some_id".to_string());
                    }
                }
            }
        } else {
            error!(self.ctx.log(), "Got unexpected message from {}", sender);
        }
    }
}

impl Require<MetricPort> for Task {
    fn handle(&mut self, _event: Metric) -> () {
        // ?
    }
}