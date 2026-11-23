//! Dynamic `<select>` option binding and temporary `<for>` item overlay for `krui-preview`.

use crate::eval::{eval_interpolated_parts, eval_template_expr};
use krui_compiler::{CompiledDynamicSelectOption, CompiledSelectChild, TemplateExpr};
use krui_core::{CallError, DynSignal, NodeId, Registry, RegistryRef, Value, ViewTree};

pub(crate) struct TempItemReg<'a> {
    pub(crate) parent: &'a dyn Registry,
    pub(crate) item_var: &'a str,
    pub(crate) item_val: &'a Value,
}

impl Registry for TempItemReg<'_> {
    fn get(&self, path: &str) -> Option<Value> {
        let trimmed = path.trim();
        if trimmed == self.item_var {
            return Some(self.item_val.clone());
        }
        if let Some(tail) = trimmed
            .strip_prefix(self.item_var)
            .and_then(|r| r.strip_prefix('.'))
        {
            if let Value::Record(rec) = self.item_val {
                return rec.get(tail);
            }
            return None;
        }
        self.parent.get(path)
    }

    fn signal(&self, path: &str) -> Option<DynSignal> {
        self.parent.signal(path)
    }

    fn call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        self.parent.call(name, args)
    }
}

pub(crate) fn eval_with_temp_item(
    expr: &TemplateExpr,
    parent: &dyn Registry,
    item_var: &str,
    item_val: &Value,
) -> Value {
    let tmp = TempItemReg {
        parent,
        item_var,
        item_val,
    };
    eval_template_expr(expr, &tmp)
}

pub(crate) fn bind_select_children_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    children: &[CompiledSelectChild],
    registry: &RegistryRef,
) {
    if children.is_empty() {
        return;
    }
    let owned = children.to_vec();
    let reg = registry.clone();
    tree.bind_select_options(node_id, move || {
        let mut out = Vec::new();
        for child in &owned {
            match child {
                CompiledSelectChild::Option(opt) => {
                    out.push(eval_dynamic_select_option(opt, reg.as_ref()));
                }
                CompiledSelectChild::For(sfor) => {
                    if let Value::List(items) = eval_template_expr(&sfor.list, reg.as_ref()) {
                        for item in &items {
                            let item_reg = TempItemReg {
                                parent: reg.as_ref(),
                                item_var: &sfor.item_var,
                                item_val: item,
                            };
                            let _ = eval_template_expr(&sfor.key, &item_reg);
                            for opt in &sfor.options {
                                out.push(eval_dynamic_select_option(opt, &item_reg));
                            }
                        }
                    }
                }
            }
        }
        out
    });
}

fn eval_dynamic_select_option(
    opt: &CompiledDynamicSelectOption,
    registry: &dyn Registry,
) -> (String, String, bool) {
    let value = eval_interpolated_parts(&opt.value_parts, registry, true);
    let label = eval_interpolated_parts(&opt.label_parts, registry, true);
    (value, label, opt.selected)
}
