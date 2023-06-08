use crate::configv2::templating::TemplatePiece;

use super::templating::{Template, TemplateType, True, OK};
use boa_engine::{
    object::{JsFunction, ObjectInitializer},
    prelude::*,
    property::Attribute,
};
use futures::{Stream, TryStreamExt};
use itertools::Itertools;
use std::collections::BTreeMap;
use zip_all::zip_all;

// TODO: Fill in Error type
type ProviderStreamStream<Ar> =
    Box<dyn Stream<Item = Result<(serde_json::Value, Vec<Ar>), ()>> + Send + Unpin + 'static>;

pub trait ProviderStream<Ar: Clone + Send + Unpin + 'static> {
    #[allow(clippy::wrong_self_convention)]
    fn into_stream(&self) -> ProviderStreamStream<Ar>;
}

struct EvalExpr {
    ctx: Context,
    efn: JsFunction,
    needed: Vec<String>,
}

impl EvalExpr {
    fn from_template<T>(template: Template<String, T, True, True>) -> Result<Self, ()>
    where
        T: TemplateType,
        T::ProvAllowed: OK,
    {
        let Template::NeedsProviders { script, .. } = template else {
            return Err(());
        };

        let mut needed = Vec::new();
        let script = format!(
            "function ____eval(____provider_values){{ return {}; }}",
            script
                .into_iter()
                .map(|p| match p {
                    TemplatePiece::Raw(s) => s,
                    TemplatePiece::Provider(p, ..) => {
                        let s = format!("____provider_values.{p}");
                        needed.push(p);
                        s
                    }
                    _ => unreachable!(),
                })
                .collect::<String>()
        );
        needed.sort();
        let mut ctx = default_context();
        ctx.eval(script).unwrap();
        let efn: JsFunction =
            JsFunction::from_object(ctx.eval("____eval").unwrap().as_object().unwrap().clone())
                .unwrap();
        Ok(Self { ctx, efn, needed })
    }

    fn into_stream<P, Ar>(
        mut self,
        providers: &BTreeMap<String, P>,
    ) -> impl Stream<Item = Result<(serde_json::Value, Vec<Ar>), ()>>
    where
        P: ProviderStream<Ar> + Sized + 'static,
        Ar: Clone + Send + Unpin + 'static,
    {
        let providers = self
            .needed
            .iter()
            .map(|pn| {
                providers
                    .get(pn)
                    .map(ProviderStream::into_stream)
                    .ok_or_else(|| format!("missing provider {pn}"))
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let prov = zip_all(providers);
        prov.map_ok(move |values| {
            let ctx = &mut self.ctx;
            let values = self
                .needed
                .iter()
                .zip(
                    values
                        .into_iter()
                        .map(|(v, ar)| (JsValue::from_json(&v, ctx).unwrap(), ar)),
                )
                .collect::<BTreeMap<_, _>>();
            let mut object = ObjectInitializer::new(ctx);
            for (name, (value, _)) in values.iter() {
                object.property(name.as_str(), value, Attribute::READONLY);
            }
            let object = object.build();
            (
                self.efn
                    .call(&JsValue::Null, &[object.into()], ctx)
                    .unwrap()
                    .to_json(ctx)
                    .unwrap(),
                values.into_iter().flat_map(|v| v.1 .1).collect_vec(),
            )
        })
    }
}

fn default_context() -> Context {
    Context::default()
}
