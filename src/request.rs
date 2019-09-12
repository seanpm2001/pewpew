mod body_handler;
mod request_maker;
mod response_handler;

use self::body_handler::BodyHandler;
use self::request_maker::RequestMaker;
use self::response_handler::ResponseHandler;

use ether::{Either, Either3};
use for_each_parallel::ForEachParallel;
use futures::{
    future::join_all, stream, sync::mpsc as futures_channel, Async, Future, IntoFuture, Sink,
    Stream,
};
use hyper::{
    client::HttpConnector,
    header::{
        Entry as HeaderEntry, HeaderMap, HeaderName, HeaderValue, CONTENT_DISPOSITION,
        CONTENT_LENGTH, CONTENT_TYPE, HOST,
    },
    Body as HyperBody, Client, Method, Request, Response,
};
use hyper_tls::HttpsConnector;
use parking_lot::Mutex;
use rand::distributions::{Alphanumeric, Distribution};
use select_any::select_any;
use serde_json as json;
use tokio::{fs::File as TokioFile, io::AsyncRead, timer::Timeout};
use zip_all::zip_all;

use crate::config::{
    self, AutoReturn, EndpointProvidesSendOptions, Select, Template, REQUEST_BODY, REQUEST_HEADERS,
    REQUEST_STARTLINE, REQUEST_URL, RESPONSE_BODY, RESPONSE_HEADERS, RESPONSE_STARTLINE, STATS,
};
use crate::error::{RecoverableError, TestError};
use crate::providers;
use crate::stats;
use crate::util::tweak_path;

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    error::Error as StdError,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    path::PathBuf,
    str,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Debug)]
pub struct TemplateValues(json::Value);

impl TemplateValues {
    pub fn new() -> Self {
        TemplateValues(json::Value::Object(json::Map::new()))
    }

    pub fn as_json(&self) -> &json::Value {
        &self.0
    }
}

impl Deref for TemplateValues {
    type Target = json::Map<String, json::Value>;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            json::Value::Object(o) => o,
            _ => unreachable!("cannot deref json value as object"),
        }
    }
}

impl DerefMut for TemplateValues {
    fn deref_mut(&mut self) -> &mut json::Map<String, json::Value> {
        match &mut self.0 {
            json::Value::Object(o) => o,
            _ => unreachable!("cannot deref json value as object"),
        }
    }
}

impl From<json::Value> for TemplateValues {
    fn from(map: json::Value) -> Self {
        TemplateValues(map)
    }
}

struct Outgoing {
    cb: Option<Arc<dyn Fn(bool) + Send + Sync + 'static>>,
    logger: bool,
    select: Arc<Select>,
    tx: channel::Sender<json::Value>,
}

impl Outgoing {
    fn new(
        select: Select,
        tx: channel::Sender<json::Value>,
        cb: Option<Arc<dyn Fn(bool) + Send + Sync + 'static>>,
        logger: bool,
    ) -> Self {
        Outgoing {
            cb,
            select: select.into(),
            tx,
            logger,
        }
    }
}

pub struct BuilderContext {
    pub config: config::Config,
    pub config_path: PathBuf,
    // the http client
    pub client: Arc<
        Client<
            HttpsConnector<HttpConnector<hyper::client::connect::dns::TokioThreadpoolGaiResolver>>,
        >,
    >,
    // a mapping of names to their prospective static (single value) providers
    pub static_vars: BTreeMap<String, json::Value>,
    // a mapping of names to their prospective providers
    pub providers: BTreeMap<String, providers::Provider>,
    // a mapping of names to their prospective loggers
    pub loggers: BTreeMap<String, (channel::Sender<json::Value>, Option<config::Select>)>,
    // channel that receives and aggregates stats for the test
    pub stats_tx: StatsTx,
}

pub struct Builder {
    body: Option<config::Body>,
    declare: BTreeMap<String, String>,
    headers: Vec<(String, String)>,
    logs: Vec<(String, Select)>,
    max_parallel_requests: Option<NonZeroUsize>,
    method: Method,
    no_auto_returns: bool,
    on_demand: bool,
    provides: Vec<(String, Select)>,
    start_stream: Option<Box<dyn Stream<Item = Instant, Error = TestError> + Send>>,
    tags: BTreeMap<String, Template>,
    url: Template,
}

impl Builder {
    pub fn new(
        url: Template,
        start_stream: Option<Box<dyn Stream<Item = Instant, Error = TestError> + Send>>,
    ) -> Self {
        Builder {
            body: None,
            declare: BTreeMap::new(),
            headers: Vec::new(),
            logs: Vec::new(),
            max_parallel_requests: None,
            method: Method::GET,
            no_auto_returns: false,
            on_demand: false,
            start_stream,
            provides: Vec::new(),
            tags: BTreeMap::new(),
            url,
        }
    }

    pub fn declare(mut self, providers: BTreeMap<String, String>) -> Self {
        self.declare.extend(providers);
        self
    }

    pub fn provides(mut self, provides: Vec<(String, Select)>) -> Self {
        self.provides = provides;
        self
    }

    pub fn logs(mut self, logs: Vec<(String, Select)>) -> Self {
        self.logs.extend(logs);
        self
    }

    pub fn max_parallel_requests(mut self, max_parallel_requests: Option<NonZeroUsize>) -> Self {
        self.max_parallel_requests = max_parallel_requests;
        self
    }

    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    pub fn no_auto_returns(mut self, no_auto_returns: bool) -> Self {
        self.no_auto_returns = no_auto_returns;
        self
    }

    pub fn on_demand(mut self, on_demand: bool) -> Self {
        self.on_demand = on_demand;
        self
    }

    pub fn headers(mut self, mut headers: Vec<(String, String)>) -> Self {
        self.headers.append(&mut headers);
        self
    }

    pub fn body(mut self, body: Option<config::Body>) -> Self {
        self.body = body;
        self
    }

    pub fn tags(mut self, tags: BTreeMap<String, Template>) -> Self {
        self.tags = tags;
        self
    }

    pub fn build(self, ctx: &mut BuilderContext) -> Result<Endpoint, TestError> {
        let mut required_providers = self.url.get_providers().clone();
        let headers = self
            .headers
            .into_iter()
            .map(|(key, v)| {
                let value = Template::new(&v, &ctx.static_vars, false)?;
                required_providers.extend(value.get_providers().clone());
                Ok::<_, TestError>((key.to_lowercase(), value))
            })
            .collect::<Result<_, _>>()?;
        let mut limits = Vec::new();
        let mut precheck_rr_providers = 0;
        let mut rr_providers = 0;
        let mut outgoing = Vec::new();
        let mut on_demand_streams: OnDemandStreams = Vec::new();
        let mut provides_set = if self.start_stream.is_none() && !self.provides.is_empty() {
            Some(BTreeSet::new())
        } else {
            None
        };
        let mut provides = Vec::new();
        for (k, v) in self.provides {
            let provider = ctx
                .providers
                .get(&k)
                .ok_or_else(|| TestError::UnknownProvider(k))?;
            let tx = provider.tx.clone();
            if let Some(set) = &mut provides_set {
                set.insert(tx.clone());
            }
            if v.get_send_behavior().is_block() {
                limits.push(tx.limit());
            }
            rr_providers |= v.get_special_providers();
            precheck_rr_providers |= v.get_where_clause_special_providers();
            required_providers.extend(v.get_providers().clone());
            let cb = if self.on_demand {
                let (stream, cb) = provider.on_demand.clone().into_stream();
                on_demand_streams.push(Box::new(stream));
                Some(cb)
            } else {
                None
            };
            provides.push(Outgoing::new(v, tx, cb, false));
        }
        let mut streams: StreamCollection = Vec::new();
        if let Some(start_stream) = self.start_stream {
            streams.push((true, Box::new(start_stream.map(|_| StreamItem::None))));
        } else if let Some(set) = provides_set {
            let stream = stream::poll_fn(move || {
                let done = set.iter().all(channel::Sender::no_receivers);
                if done {
                    Ok(Async::Ready(None))
                } else {
                    Ok(Async::Ready(Some(StreamItem::None)))
                }
            });
            streams.push((true, Box::new(stream)));
        }
        for (k, v) in self.logs {
            let (tx, _) = ctx
                .loggers
                .get(&k)
                .ok_or_else(|| TestError::UnknownLogger(k))?;
            rr_providers |= v.get_special_providers();
            precheck_rr_providers |= v.get_where_clause_special_providers();
            required_providers.extend(v.get_providers().clone());
            outgoing.push(Outgoing::new(v, tx.clone(), None, true));
        }
        outgoing.extend(ctx.loggers.values().filter_map(|(tx, select)| {
            if let Some(select) = select {
                required_providers.extend(select.get_providers().clone());
                rr_providers |= select.get_special_providers();
                precheck_rr_providers |= select.get_where_clause_special_providers();
                Some(Outgoing::new(select.clone(), tx.clone(), None, true))
            } else {
                None
            }
        }));
        let body = self
            .body
            .map(|body| {
                let value = match body {
                    config::Body::File(body) => {
                        let template = Template::new(&body, &ctx.static_vars, false)?;
                        required_providers.extend(template.get_providers().clone());
                        BodyTemplate::File(ctx.config_path.clone(), template)
                    }
                    config::Body::String(body) => {
                        let template = Template::new(&body, &ctx.static_vars, false)?;
                        required_providers.extend(template.get_providers().clone());
                        BodyTemplate::String(template)
                    }
                    config::Body::Multipart(multipart) => {
                        let pieces = multipart
                            .into_iter()
                            .map(|(name, v)| {
                                let (is_file, template) = match v.body {
                                    config::BodyMultipartPieceBody::File(f) => {
                                        let template = Template::new(&f, &ctx.static_vars, false)?;
                                        required_providers.extend(template.get_providers().clone());
                                        (true, template)
                                    }
                                    config::BodyMultipartPieceBody::String(s) => {
                                        let template = Template::new(&s, &ctx.static_vars, false)?;
                                        required_providers.extend(template.get_providers().clone());
                                        (false, template)
                                    }
                                };
                                let headers = v
                                    .headers
                                    .into_iter()
                                    .map(|(k, v)| {
                                        let template = Template::new(&v, &ctx.static_vars, false)?;
                                        required_providers.extend(template.get_providers().clone());
                                        Ok::<_, TestError>((k, template))
                                    })
                                    .collect::<Result<_, _>>()?;

                                let piece = MultipartPiece {
                                    name,
                                    headers,
                                    is_file,
                                    template,
                                };
                                Ok::<_, TestError>(piece)
                            })
                            .collect::<Result<_, _>>()?;
                        let multipart = MultipartBody {
                            path: ctx.config_path.clone(),
                            pieces,
                        };
                        BodyTemplate::Multipart(multipart)
                    }
                };
                Ok::<_, TestError>(value)
            })
            .transpose()?
            .unwrap_or(BodyTemplate::None);
        let mut required_providers2 = BTreeSet::new();
        for (name, d) in self.declare {
            required_providers.remove(&name);
            let vce = config::ValueOrExpression::new(
                &d,
                &mut required_providers2,
                &ctx.static_vars,
                false,
            )?;
            let stream = vce
                .into_stream(&ctx.providers, false)
                .map(move |(v, returns)| StreamItem::Declare(name.clone(), v, returns));
            streams.push((false, Box::new(stream)));
        }
        let no_auto_returns = self.no_auto_returns;
        // go through the list of required providers and make sure we have them all
        for name in &required_providers {
            let provider = ctx
                .providers
                .get(name)
                .ok_or_else(|| TestError::UnknownProvider(name.clone()))?;
            let receiver = provider.rx.clone();
            let ar = provider
                .auto_return
                .map(|send_option| (send_option, provider.tx.clone()));
            let name = name.clone();
            let provider_stream = Box::new(
                Stream::map(receiver, move |v| {
                    let ar = if no_auto_returns {
                        None
                    } else {
                        ar.clone().map(|(send_option, tx)| {
                            AutoReturn::new(send_option, tx, vec![v.clone()])
                        })
                    };
                    StreamItem::TemplateValue(name.clone(), v, ar)
                })
                .map_err(|_| TestError::Internal("Unexpected error from receiver".into())),
            );
            streams.push((false, provider_stream));
        }
        required_providers.extend(required_providers2);
        let stats_tx = ctx.stats_tx.clone();
        let client = ctx.client.clone();
        let method = self.method;
        let timeout = ctx.config.client.request_timeout;
        Ok(Endpoint {
            body,
            client,
            headers,
            limits,
            max_parallel_requests: self.max_parallel_requests,
            method,
            no_auto_returns,
            on_demand_streams,
            outgoing,
            precheck_rr_providers,
            provides,
            required_providers,
            rr_providers,
            tags: Arc::new(self.tags),
            stats_tx,
            stream_collection: streams,
            url: self.url,
            timeout,
        })
    }
}

enum StreamItem {
    Declare(String, json::Value, Vec<config::AutoReturn>),
    None,
    TemplateValue(String, json::Value, Option<config::AutoReturn>),
}

struct MultipartPiece {
    name: String,
    headers: Vec<(String, Template)>,
    is_file: bool,
    template: Template,
}

struct MultipartBody {
    path: PathBuf,
    pieces: Vec<MultipartPiece>,
}

impl MultipartBody {
    fn as_hyper_body<'a>(
        &self,
        template_values: &TemplateValues,
        content_type_entry: HeaderEntry<'a, HeaderValue>,
        copy_body_value: bool,
        body_value: &mut Option<String>,
    ) -> impl Future<Item = (u64, HyperBody), Error = TestError> {
        let boundary: String = Alphanumeric
            .sample_iter(&mut rand::thread_rng())
            .take(20)
            .collect();

        let is_form = {
            let content_type = content_type_entry
                .or_insert_with(|| HeaderValue::from_static("multipart/form-data"));
            let ct_str = match content_type.to_str() {
                Ok(c) => c,
                Err(e) => {
                    return Either::A(
                        Err(RecoverableError::BodyErr(Arc::new(e)).into()).into_future(),
                    )
                }
            };
            if ct_str.starts_with("multipart/") {
                let is_form = ct_str.starts_with("multipart/form-data");
                *content_type =
                    match HeaderValue::from_str(&format!("{};boundary={}", ct_str, boundary)) {
                        Ok(c) => c,
                        Err(e) => {
                            return Either::A(
                                Err(RecoverableError::BodyErr(Arc::new(e)).into()).into_future(),
                            )
                        }
                    };
                is_form
            } else {
                *content_type = match HeaderValue::from_str(&format!(
                    "multipart/form-data;boundary={}",
                    boundary
                )) {
                    Ok(c) => c,
                    Err(e) => {
                        return Either::A(
                            Err(RecoverableError::BodyErr(Arc::new(e)).into()).into_future(),
                        )
                    }
                };
                true
            }
        };

        let mut closing_boundary = Vec::new();
        closing_boundary.extend_from_slice(b"\r\n--");
        closing_boundary.extend_from_slice(boundary.as_bytes());
        closing_boundary.extend_from_slice(b"--\r\n");

        let mut body_value2 = Vec::new();

        let pieces: Vec<_> = self
            .pieces
            .iter()
            .enumerate()
            .map(|(i, mp)| {
                let mut body = match mp
                    .template
                    .evaluate(Cow::Borrowed(template_values.as_json()), None)
                {
                    Ok(b) => b,
                    Err(e) => return Either3::A(Err(e).into_future()),
                };

                let mut has_content_disposition = false;

                let mut piece_data = Vec::new();
                if i == 0 {
                    piece_data.extend_from_slice(b"--");
                } else {
                    piece_data.extend_from_slice(b"\r\n--");
                }
                piece_data.extend_from_slice(boundary.as_bytes());

                for (k, t) in mp.headers.iter() {
                    let key = match HeaderName::from_bytes(k.as_bytes()) {
                        Ok(k) => k,
                        Err(e) => {
                            return Either3::A(
                                Err(RecoverableError::BodyErr(Arc::new(e)).into()).into_future(),
                            )
                        }
                    };
                    let value = match t.evaluate(Cow::Borrowed(template_values.as_json()), None) {
                        Ok(v) => v,
                        Err(e) => return Either3::A(Err(e).into_future()),
                    };
                    let value = match HeaderValue::from_str(&value) {
                        Ok(v) => v,
                        Err(e) => {
                            return Either3::A(
                                Err(RecoverableError::BodyErr(Arc::new(e)).into()).into_future(),
                            )
                        }
                    };

                    let content_disposition = CONTENT_DISPOSITION;
                    has_content_disposition |= key == content_disposition;

                    piece_data.extend_from_slice(b"\r\n");
                    piece_data.extend_from_slice(key.as_ref());
                    piece_data.extend_from_slice(b": ");
                    piece_data.extend_from_slice(value.as_bytes());
                }

                if is_form && !has_content_disposition {
                    let value = if mp.is_file {
                        HeaderValue::from_str(&format!(
                            "form-data; name=\"{}\"; filename=\"{}\"",
                            mp.name, body
                        ))
                    } else {
                        HeaderValue::from_str(&format!("form-data; name=\"{}\"", mp.name))
                    };
                    let value = match value {
                        Ok(v) => v,
                        Err(e) => {
                            return Either3::A(
                                Err(RecoverableError::BodyErr(Arc::new(e)).into()).into_future(),
                            )
                        }
                    };

                    piece_data.extend_from_slice(b"\r\ncontent-disposition: ");
                    piece_data.extend_from_slice(value.as_bytes());
                }

                piece_data.extend_from_slice(b"\r\n\r\n");

                if mp.is_file {
                    if copy_body_value {
                        body_value2.extend_from_slice(&piece_data);
                        body_value2.extend_from_slice(b"<<contents of file: ");
                        body_value2.extend_from_slice(body.as_bytes());
                        body_value2.extend_from_slice(b">>");
                    }
                    let piece_data_bytes = piece_data.len() as u64;
                    let piece_stream = stream::once(Ok(hyper::Chunk::from(piece_data)));
                    tweak_path(&mut body, &self.path);
                    let b = create_file_hyper_body(body).map(move |(bytes, body)| {
                        let stream = Either::A(piece_stream.chain(body));
                        (bytes + piece_data_bytes, stream)
                    });
                    Either3::B(b)
                } else {
                    piece_data.extend_from_slice(body.as_bytes());
                    if copy_body_value {
                        body_value2.extend_from_slice(&piece_data);
                    }
                    let piece_data_bytes = piece_data.len() as u64;
                    let piece_stream = stream::once(Ok(hyper::Chunk::from(piece_data)));
                    let c = Ok((piece_data_bytes, Either::B(piece_stream)));
                    Either3::C(c.into_future())
                }
            })
            .collect();

        if copy_body_value {
            body_value2.extend_from_slice(&closing_boundary);
            let bv = match String::from_utf8(body_value2) {
                Ok(bv) => bv,
                Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
            };
            *body_value = Some(bv);
        }

        let b = join_all(pieces).map(move |results| {
            let (bytes, bodies) = results.into_iter().fold(
                (closing_boundary.len() as u64, Vec::new()),
                |(bytes, mut bodies), (bytes2, body)| {
                    bodies.push(body);
                    (bytes + bytes2, bodies)
                },
            );

            let closing_boundary = hyper::Chunk::from(closing_boundary.clone());

            let stream = stream::iter_ok::<_, hyper::Error>(bodies)
                .flatten()
                .chain(stream::once(Ok(closing_boundary)));

            (bytes, HyperBody::wrap_stream(stream))
        });
        Either::B(b)
    }
}

fn create_file_hyper_body(file: String) -> impl Future<Item = (u64, HyperBody), Error = TestError> {
    TokioFile::open(file)
        .and_then(TokioFile::metadata)
        .map(|(mut file, metadata)| {
            let bytes = metadata.len();
            let mut buf = bytes::BytesMut::with_capacity(8 * (1 << 10));
            let stream = stream::poll_fn(move || {
                buf.reserve(8 * (1 << 10));
                let ret = match file.read_buf(&mut buf)? {
                    Async::Ready(n) if n == 0 => Async::Ready(None),
                    Async::Ready(_) => Async::Ready(buf.take().freeze().into()),
                    Async::NotReady => Async::NotReady,
                };
                Ok::<_, tokio::io::Error>(ret)
            });

            let body = HyperBody::wrap_stream(stream);
            (bytes, body)
        })
        .map_err(|e| TestError::Recoverable(RecoverableError::BodyErr(Arc::new(e))))
}

enum BodyTemplate {
    File(PathBuf, Template),
    Multipart(MultipartBody),
    None,
    String(Template),
}

impl BodyTemplate {
    fn as_hyper_body<'a>(
        &self,
        template_values: &TemplateValues,
        copy_body_value: bool,
        body_value: &mut Option<String>,
        content_type_entry: HeaderEntry<'a, HeaderValue>,
    ) -> impl Future<Item = (u64, HyperBody), Error = TestError> {
        let template = match self {
            BodyTemplate::File(_, t) => t,
            BodyTemplate::Multipart(m) => {
                return Either3::A(m.as_hyper_body(
                    template_values,
                    content_type_entry,
                    copy_body_value,
                    body_value,
                ))
            }
            BodyTemplate::None => return Either3::B(Ok((0, HyperBody::empty())).into_future()),
            BodyTemplate::String(t) => t,
        };
        let mut body = match template.evaluate(Cow::Borrowed(template_values.as_json()), None) {
            Ok(b) => b,
            Err(e) => return Either3::B(Err(e).into_future()),
        };
        if let BodyTemplate::File(path, _) = self {
            tweak_path(&mut body, path);
            if copy_body_value {
                *body_value = Some(format!("<contents of file: {}>", body));
            }
            Either3::C(create_file_hyper_body(body))
        } else {
            if copy_body_value {
                *body_value = Some(body.clone());
            }
            Either3::B(Ok((body.as_bytes().len() as u64, body.into())).into_future())
        }
    }
}

type StreamCollection = Vec<(
    bool,
    Box<dyn Stream<Item = StreamItem, Error = TestError> + Send + 'static>,
)>;
type OnDemandStreams = Vec<Box<dyn Stream<Item = (), Error = ()> + Send + 'static>>;
pub type StatsTx = futures_channel::UnboundedSender<stats::StatsMessage>;

pub struct Endpoint {
    body: BodyTemplate,
    client: Arc<
        Client<
            HttpsConnector<HttpConnector<hyper::client::connect::dns::TokioThreadpoolGaiResolver>>,
        >,
    >,
    headers: Vec<(String, Template)>,
    limits: Vec<channel::Limit>,
    max_parallel_requests: Option<NonZeroUsize>,
    method: Method,
    no_auto_returns: bool,
    on_demand_streams: OnDemandStreams,
    outgoing: Vec<Outgoing>,
    precheck_rr_providers: u16,
    provides: Vec<Outgoing>,
    rr_providers: u16,
    required_providers: BTreeSet<String>,
    tags: Arc<BTreeMap<String, Template>>,
    stats_tx: StatsTx,
    stream_collection: StreamCollection,
    timeout: Duration,
    url: Template,
}

impl Endpoint {
    pub fn required_providers(&self) -> &BTreeSet<String> {
        &self.required_providers
    }

    pub fn clear_provides(&mut self) {
        self.provides.clear();
    }

    pub fn add_start_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = (), Error = TestError> + Send + 'static,
    {
        let stream = Box::new(stream.map(|_| StreamItem::None));
        match self.stream_collection.get_mut(0) {
            Some((true, s)) => {
                *s = stream;
            }
            _ => self.stream_collection.push((true, stream)),
        }
    }

    // This returns a boxed future because otherwise the type system runs out of memory for the type
    pub fn into_future(self) -> Box<dyn Future<Item = (), Error = TestError> + Send> {
        let url = self.url;
        let method = self.method;
        let headers = self.headers;
        let body = self.body;
        let rr_providers = self.rr_providers;
        let client = self.client;
        let stats_tx = self.stats_tx;
        let no_auto_returns = self.no_auto_returns;
        let streams = self.stream_collection.into_iter().map(|t| t.1);
        let stream = if !self.on_demand_streams.is_empty() && !self.provides.is_empty() {
            let mut on_demand_streams = select_any(self.on_demand_streams);
            let mut zipped_streams = zip_all(streams);
            let mut od_continue = false;
            let stream = stream::poll_fn(move || {
                let p = on_demand_streams.poll();
                if !od_continue {
                    match p {
                        Ok(Async::Ready(Some(_))) => od_continue = true,
                        Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                        Ok(Async::NotReady) => return Ok(Async::NotReady),
                        Err(_) => {
                            return Err(TestError::Internal(
                                "on demand streams should never error".into(),
                            ));
                        }
                    }
                }
                let p = zipped_streams.poll();
                match p {
                    Ok(Async::NotReady) => (),
                    _ => {
                        od_continue = false;
                    }
                }
                p
            });
            Either::A(stream)
        } else {
            Either::B(zip_all(streams))
        };
        let mut outgoing = self.outgoing;
        outgoing.extend(self.provides);
        let outgoing = Arc::new(outgoing);
        let precheck_rr_providers = self.precheck_rr_providers;
        let timeout = self.timeout;
        let limits = self.limits;
        let max_parallel_requests = self.max_parallel_requests;
        let tags = self.tags;
        let rm = RequestMaker {
            url,
            method,
            headers,
            body,
            rr_providers,
            client,
            stats_tx,
            no_auto_returns,
            outgoing,
            precheck_rr_providers,
            tags,
            timeout,
        };
        Box::new(ForEachParallel::new(
            limits,
            max_parallel_requests,
            stream,
            move |values| rm.send_request(values),
        ))
    }
}

struct BlockSender<V: Iterator<Item = Result<json::Value, TestError>>> {
    cb: Option<
        std::sync::Arc<(dyn std::ops::Fn(bool) + std::marker::Send + std::marker::Sync + 'static)>,
    >,
    last_value: Option<json::Value>,
    tx: channel::Sender<serde_json::value::Value>,
    value_added: bool,
    values: V,
}

impl<V: Iterator<Item = Result<json::Value, TestError>>> BlockSender<V> {
    fn new(
        values: V,
        tx: channel::Sender<serde_json::value::Value>,
        cb: Option<
            std::sync::Arc<
                (dyn std::ops::Fn(bool) + std::marker::Send + std::marker::Sync + 'static),
            >,
        >,
    ) -> Self {
        BlockSender {
            cb,
            last_value: None,
            tx,
            value_added: false,
            values,
        }
    }
}

impl<V: Iterator<Item = Result<json::Value, TestError>>> Future for BlockSender<V> {
    type Item = ();
    type Error = TestError;

    fn poll(&mut self) -> Result<Async<()>, TestError> {
        loop {
            let v = if let Some(v) = self.last_value.take() {
                v
            } else if let Some(r) = self.values.next() {
                r?
            } else {
                return Ok(Async::Ready(()));
            };
            match self.tx.try_send(v) {
                channel::SendState::Closed => return Ok(Async::Ready(())),
                channel::SendState::Full(v) => {
                    self.last_value = Some(v);
                    return Ok(Async::NotReady);
                }
                channel::SendState::Success => {
                    self.value_added = true;
                }
            }
        }
    }
}

impl<V: Iterator<Item = Result<json::Value, TestError>>> Drop for BlockSender<V> {
    fn drop(&mut self) {
        let _ = self.poll();
        if let Some(cb) = &self.cb {
            cb(self.value_added);
        }
    }
}
