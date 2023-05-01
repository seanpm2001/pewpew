mod byte_utils;
mod error;
mod message;

use byte_utils::decode_incoming_bytes;
use deno_core::{JsRuntime, OpState};
use futures::{Stream, StreamExt};
use message::{Header, IncomingMessage, IncomingResponse, OutgoingMessage};
use serde_json as json;
use tokio::{io::AsyncWriteExt, process};

use std::{cell::RefCell, collections::BTreeMap, process::Stdio, rc::Rc};

pub struct JsScripting {
    op_state: Rc<RefCell<OpState>>,
    // providers: BTreeMap<String, Provider>,
}

pub struct JsFunctions {
    pre_fns: Vec<String>,
    post_fns: Vec<String>,
    context: String,
}

pub struct JsError;

impl JsFunctions {
    pub fn add_endpoint_pre_fn(&mut self, fn_body: String) -> Result<usize, JsError> {
        todo!("append function header and parse into AST to check for correctness");
        let i = self.pre_fns.len();
        self.pre_fns.push(fn_body);
        Ok(i)
    }

    pub fn add_endpoint_post_fn(&mut self, fn_body: String) -> Result<usize, JsError> {
        todo!("append function header and parse into AST to check for correctness");
        let i = self.post_fns.len();
        self.post_fns.push(fn_body);
        Ok(i)
    }

    pub fn add_context(&mut self, context: String) -> Result<(), JsError> {
        todo!("parse context into AST to check for correctness");
        self.context = context;
        Ok(())
    }
}

thread_local! {
    static JS_RUNTIME: RefCell<Option<JsRuntime>> = RefCell::new(None);
}

impl JsScripting {
    pub fn new(fns: JsFunctions) -> JsScripting {
        let op_state = JS_RUNTIME.with(|runtime| {
            let mut runtime = runtime.borrow_mut();
            if runtime.is_none() {
                runtime.get_or_insert_with(|| JsRuntime::new(Default::default()));
                todo!("add ops");
            }
            runtime.as_mut().unwrap().op_state()
        });
        todo!("generate code and execute on JS runtime");
        JsScripting { op_state }
    }

    pub async fn call_endpoint_pre_fn(&mut self, fn_id: usize) -> Option<json::Value> {
        // For an example of calling route_op see
        // https://github.com/denoland/deno/blob/055dfe2ff437099aef105fe4beab0f0c8cc53506/core/bindings.rs#L430
        unimplemented!()
    }

    pub async fn call_endpoint_post_fn(
        &mut self,
        fn_id: usize,
        request: json::Value,
        response: json::Value,
    ) {
        unimplemented!()
    }
}

pub struct Demo {
    incoming: Box<dyn Stream<Item = Result<(Header, IncomingMessage), error::Error>> + Unpin>,
    outgoing: process::ChildStdin,
}

impl Demo {
    pub fn new() -> Self {
        let child = process::Command::new("deno")
            // .current_dir("./deno")
            .args(&["run", "--unstable", "--allow-read", "deno/pewpew.ts"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let child_in = child.stdin.unwrap();
        let child_out = child.stdout.unwrap();

        let incoming = Box::new(decode_incoming_bytes(child_out));
        let outgoing = child_in;

        Self { incoming, outgoing }
    }

    pub async fn send_out_message(
        &mut self,
        outgoing_message: OutgoingMessage,
    ) -> IncomingResponse {
        let (header_bytes, body_bytes, request_id) = outgoing_message.encode();
        self.outgoing.write_all(&header_bytes).await.unwrap();
        self.outgoing.write_all(&body_bytes).await.unwrap();
        while let Some(Ok((header, incoming_message))) = self.incoming.next().await {
            if let (IncomingMessage::IncomingResponse(r), true) =
                (incoming_message, header.id == request_id)
            {
                return r;
            }
        }
        unreachable!("shouldn't happen");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::CreateFunctionResult;

    #[tokio::test]
    async fn create_and_execute_js_function() {
        println!("create demo");
        let mut demo = Demo::new();
        println!("after create demo");

        let create_function = OutgoingMessage::CreateEndpointPreFunction(
            r#"
            // let mod = await import("./eval.ts");
            const a = [1, 2, 3];
            console.error("hello from Deno", a);
            // let value = await providers.get("name");
            return { foo: 1234, bar: "abc" };
        "#
            .to_string(),
        );

        println!("create function");
        let fn_id = match demo.send_out_message(create_function).await {
            IncomingResponse::CreateEndpointPreFunction(CreateFunctionResult::Id(fn_id)) => fn_id,
            IncomingResponse::CreateEndpointPreFunction(CreateFunctionResult::Error(e)) => {
                panic!("{}", e);
            }
            _ => unreachable!(),
        };
        println!("after create function");

        println!("call function");
        let call_function = OutgoingMessage::CallEndpointPreFunction(fn_id);
        let return_data = match demo.send_out_message(call_function).await {
            IncomingResponse::CallEndpointPreFunction(result) => result,
            _ => unreachable!(),
        };
        println!("after call function");
        dbg!(return_data);
    }
}
