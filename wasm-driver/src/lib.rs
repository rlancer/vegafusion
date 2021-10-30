mod utils;

use prost::Message;

use vegafusion_core::arrow::array::Float64Array;

// use vegafusion_core::expression::parser::parse;
use vegafusion_core::proto::gen::expression;
use vegafusion_core::data::scalar::{ScalarValue, ScalarValueHelpers};
use wasm_bindgen::prelude::*;
use vegafusion_core::proto::gen::tasks::{TaskValue as ProtoTaskValue, Variable, Task, TaskGraph, DataSourceTask, DataUrlTask, VariableNamespace, TaskNode, TaskGraphValueRequest, NodeValueIndex, TaskGraphValueResponse};
use vegafusion_core::task_graph::task_value::TaskValue;
use std::convert::TryFrom;
use vegafusion_core::task_graph::scope::TaskScope;
use vegafusion_core::proto::gen::transforms::{TransformPipeline, Transform, Extent};
use vegafusion_core::proto::gen::transforms::transform::TransformKind;
use vegafusion_core::proto::gen::tasks::data_url_task::Url;
use vegafusion_core::error::Result;
use js_sys::JSON::stringify;
use js_sys::{JsString, Uint8Array};
use serde_json::{json, Value};

use wasm_bindgen::prelude::*;
use vegafusion_core::spec::chart::ChartSpec;
use web_sys::{HtmlElement, Element};
use vegafusion_core::planning::extract::extract_server_data;
use vegafusion_core::planning::stitch::{stitch_specs, CommPlan};
use std::sync::{Arc, Mutex};
use vegafusion_core::proto::gen::services::{
    VegaFusionRuntimeRequest, vega_fusion_runtime_request, VegaFusionRuntimeResponse, vega_fusion_runtime_response
};
use vegafusion_core::data::table::VegaFusionTable;
use std::collections::{HashMap, HashSet};
use vegafusion_core::task_graph::task_graph::ScopedVariable;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    fn alert(s: &str);

    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}


#[wasm_bindgen]
pub struct MsgReceiver {
    element_id: String,
    spec: ChartSpec,
    comm_plan: CommPlan,
    send_msg_fn: Arc<js_sys::Function>,
    task_graph: Arc<Mutex<TaskGraph>>,
    task_graph_mapping: HashMap<ScopedVariable, NodeValueIndex>,
    server_to_client_value_indices: Arc<HashSet<NodeValueIndex>>,
    view: View,
}

#[wasm_bindgen]
impl MsgReceiver {
    fn new(element_id: &str, spec: ChartSpec, comm_plan: CommPlan, task_graph: TaskGraph, send_msg_fn: js_sys::Function) -> Self {
        let task_graph_mapping = task_graph.build_mapping();

        let server_to_client_value_indices: Arc<HashSet<_>> = Arc::new(comm_plan.server_to_client.iter().map(|scoped_var| {
            task_graph_mapping.get(scoped_var).unwrap().clone()
        }).collect());

        // Mount vega chart
        let window: web_sys::Window = web_sys::window().expect("no global `window` exists");
        let document: web_sys::Document = window.document().expect("should have a document on window");
        let mount_element = document.get_element_by_id(&element_id).unwrap();

        // log(&format!("client spec\n:{}", serde_json::to_string_pretty(&spec).unwrap()));
        let dataflow = parse(JsValue::from_serde(&spec).unwrap());

        let view = View::new(dataflow);
        view.initialize(mount_element);
        view.hover();

        let this = Self {
            element_id: element_id.to_string(),
            spec,
            comm_plan,
            task_graph: Arc::new(Mutex::new(task_graph)),
            task_graph_mapping,
            send_msg_fn: Arc::new(send_msg_fn),
            server_to_client_value_indices,
            view
        };

        this.register_callbacks();

        this
    }

    fn init_spec(&mut self, task_graph_vals: &TaskGraphValueResponse) -> Result<()> {
        for response_val in &task_graph_vals.response_values {
            let value = TaskValue::try_from(response_val.value.as_ref().unwrap()).unwrap();
            let scope = &response_val.scope;
            let var = response_val.variable.as_ref().unwrap();

            match value {
                TaskValue::Scalar(value) => {
                    let sig = self.spec.get_nested_signal_mut(scope.as_slice(), &var.name)?;
                    sig.value = Some(value.to_json()?);
                }
                TaskValue::Table(value) => {
                    let data = self.spec.get_nested_data_mut(scope.as_slice(), &var.name)?;
                    data.values = Some(value.to_json());
                }
            }
        }

        Ok(())
    }

    pub fn receive(&mut self, bytes: Vec<u8>) {
        // Decode message
        let response = VegaFusionRuntimeResponse::decode(bytes.as_slice()).unwrap();
        // log(&format!("Received msg: {:?}", response));

        if let Some(response) = response.response {
            match response {
                vega_fusion_runtime_response::Response::TaskGraphValues(task_graph_vals) => {
                    let view = self.view();
                    for response_val in task_graph_vals.response_values {
                        let value = response_val.value.unwrap();
                        let scope = response_val.scope;
                        let var = response_val.variable.unwrap();

                        // Convert from proto task value to task value
                        let value = TaskValue::try_from(&value).unwrap();

                        match value {
                            TaskValue::Scalar(value) => {
                                let js_value = JsValue::from_serde(&value.to_json().unwrap()).unwrap();
                                view.set_signal(&var.name, js_value);
                            }
                            TaskValue::Table(value) => {
                                let js_value = JsValue::from_serde(
                                    &value.to_json()
                                ).unwrap();
                                view.set_data(&var.name, js_value);
                            }
                        }
                    }
                    view.run();
                }
            }
        }
    }

    fn view(&self) -> &View {
        // self.view.as_ref().expect("View not initialized")
        &self.view
    }

    fn add_signal_listener(&self, name: &str, handler: JsValue) {
        self.view().add_signal_listener(name, handler);
    }

    fn add_data_listener(&self, name: &str, handler: JsValue) {
        self.view().add_data_listener(name, handler);
    }

    fn register_callbacks(&self) {
        for scoped_var in &self.comm_plan.client_to_server {
            let var_name = scoped_var.0.name.clone();
            let node_value_index = self.task_graph_mapping.get(&scoped_var).unwrap().clone();
            let server_to_client = self.server_to_client_value_indices.clone();

            let task_graph = self.task_graph.clone();
            let send_msg_fn = self.send_msg_fn.clone();

            // Register callbacks
            match scoped_var.0.namespace() {
                VariableNamespace::Signal => {
                    let closure = Closure::wrap(Box::new(move |name: String, val: JsValue| {
                        let val: serde_json::Value = val.into_serde().unwrap();
                        let mut task_graph = task_graph.lock().unwrap();
                        let updated_nodes = &task_graph.update_value(
                            node_value_index.node_index as usize,
                            TaskValue::Scalar(ScalarValue::from_json(&val).unwrap())
                        ).unwrap();

                        // Filter to update nodes in the comm plan
                        let updated_nodes: Vec<_> = updated_nodes.iter().cloned().filter(|node| {
                            server_to_client.contains(node)
                        }).collect();

                        let request_msg = VegaFusionRuntimeRequest {
                            request: Some(vega_fusion_runtime_request::Request::TaskGraphValues(
                                TaskGraphValueRequest {
                                    task_graph: Some(task_graph.clone()),
                                    indices: updated_nodes.clone()
                                }
                            ) )
                        };

                        Self::send_request(send_msg_fn.as_ref(), request_msg);
                    }) as Box<dyn FnMut(String, JsValue)>);

                    let ret_cb = closure.as_ref().clone();
                    closure.forget();

                    self.add_signal_listener(&var_name, ret_cb);
                }
                VariableNamespace::Data => {
                    let closure = Closure::wrap(Box::new(move |name: String, val: JsValue| {
                        let val: serde_json::Value = val.into_serde().unwrap();
                        let mut task_graph = task_graph.lock().expect("lock task graph");
                        let updated_nodes = &task_graph.update_value(
                            node_value_index.node_index as usize,
                            TaskValue::Table(
                                VegaFusionTable::from_json(&val, 1024).unwrap()
                            )
                        ).unwrap();

                        // Filter to update nodes in the comm plan
                        let updated_nodes: Vec<_> = updated_nodes.iter().cloned().filter(|node| {
                            server_to_client.contains(node)
                        }).collect();

                        let request_msg = VegaFusionRuntimeRequest {
                            request: Some(vega_fusion_runtime_request::Request::TaskGraphValues(
                                TaskGraphValueRequest {
                                    task_graph: Some(task_graph.clone()),
                                    indices: updated_nodes.clone(),
                                }
                            ))
                        };

                        Self::send_request(send_msg_fn.as_ref(), request_msg);
                    }) as Box<dyn FnMut(String, JsValue)>);

                    let ret_cb = closure.as_ref().clone();
                    closure.forget();

                    self.add_data_listener(&var_name, ret_cb);
                }
                _ => panic!("Unsupported namespace")
            }
        }
    }

    fn send_request(send_msg_fn: &js_sys::Function, request_msg: VegaFusionRuntimeRequest) {
        let mut buf: Vec<u8> = Vec::new();
        buf.reserve(request_msg.encoded_len());
        request_msg.encode(&mut buf).unwrap();

        let context: JsValue = JsValue::from_serde(&serde_json::Value::Null).unwrap();
        let js_buffer = js_sys::Uint8Array::from(buf.as_slice());
        send_msg_fn.call1(&context, &js_buffer);
    }

    fn initial_node_value_indices(&self) -> Vec<NodeValueIndex> {
        self.comm_plan.server_to_client.iter().map(|scoped_var| {
            self.task_graph_mapping.get(scoped_var).unwrap().clone()
        }).collect()
    }
}


#[wasm_bindgen]
pub fn render_vegafusion(element_id: &str, spec_str: &str, send_msg_fn: js_sys::Function) -> MsgReceiver {
    let mut spec: ChartSpec = serde_json::from_str(spec_str).unwrap();

    // Get full spec's scope
    let task_scope = spec.to_task_scope().unwrap();

    let mut server_spec = extract_server_data(&mut spec).unwrap();
    let comm_plan = stitch_specs(&task_scope, &mut server_spec, &mut spec).unwrap();

    let tasks = server_spec.to_tasks().unwrap();
    let task_graph = TaskGraph::new(tasks, &task_scope).unwrap();

    // Create closure to update chart from received messages
    let mut receiver = MsgReceiver::new(element_id, spec, comm_plan, task_graph.clone(), send_msg_fn);

    // Request initial values
    let mut updated_node_indices: Vec<_> = receiver.initial_node_value_indices();

    let request_msg = VegaFusionRuntimeRequest {
        request: Some(vega_fusion_runtime_request::Request::TaskGraphValues(
            TaskGraphValueRequest {
                task_graph: Some(task_graph),
                indices: updated_node_indices
            }
        ) )
    };

    MsgReceiver::send_request(receiver.send_msg_fn.as_ref(), request_msg);

    receiver
}


#[wasm_bindgen(module = "/js/vega_utils.js")]
extern "C" {
    fn vega_version() -> String;
}

#[wasm_bindgen(module = "vega")]
extern "C" {
    pub fn parse(spec: JsValue) -> JsValue;

    pub type View;

    #[wasm_bindgen(constructor)]
    pub fn new(dataflow: JsValue) -> View;

    #[wasm_bindgen(method, js_name="signal")]
    pub fn get_signal(this: &View, signal: &str) -> JsValue;

    #[wasm_bindgen(method, js_name="signal")]
    pub fn set_signal(this: &View, signal: &str, value: JsValue);

    #[wasm_bindgen(method, js_name="data")]
    pub fn get_data(this: &View, signal: &str) -> JsValue;

    #[wasm_bindgen(method, js_name="data")]
    pub fn set_data(this: &View, signal: &str, value: JsValue);

    #[wasm_bindgen(method, js_name="addSignalListener")]
    pub fn add_signal_listener(this: &View, name: &str, handler: JsValue);

    #[wasm_bindgen(method, js_name="addDataListener")]
    pub fn add_data_listener(this: &View, name: &str, handler: JsValue);

    #[wasm_bindgen(method, js_name="initialize")]
    pub fn initialize(this: &View, container: Element);

    #[wasm_bindgen(method, js_name="run")]
    pub fn run(this: &View);

    #[wasm_bindgen(method, js_name="hover")]
    pub fn hover(this: &View);
}


#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
