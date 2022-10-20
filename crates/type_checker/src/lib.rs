use ast::{Ast, AstNode, AstVisitor, Literal, Visitable};
use common::{Operator, Type};
use symbol_table::{Symbol, SymbolTable};

#[macro_use]
extern crate common;

mod macros;
#[cfg(test)]
mod tests;

// Performs the following tasks:
// - applies types to all nodes
// - checks for annotation consistency
// - checks for type consistency and relevance in binops
// - checks for type consistency in for step
// - checks for type consistency in if branches
// - checks main()'s annotation
// - checks for unknown functions and variables
// - initializes uninitialized variables

pub struct TypeChecker<'a> {
    symbol_table: &'a mut SymbolTable<Symbol>,
    hint: Option<Type>,
}

impl<'a> TypeChecker<'a> {
    pub fn new(symbol_table: &'a mut SymbolTable<Symbol>) -> Self {
        TypeChecker { symbol_table, hint: None }
    }

    pub fn walk(mut self, ast: Ast<AstNode>) -> Result<Ast<AstNode>, String> {
        let mut typed_ast = Ast::new();
        for node in ast.into_nodes() {
            let typed_node = node.accept(&mut self)?;
            typed_ast.add(typed_node)
        }
        Ok(typed_ast)
    }

    // Wrapper for `visit_node()` to handle hint updates
    fn check_node(&mut self, node: AstNode, hint: Option<&Type>) -> Result<AstNode, String> {
        self.hint = hint.cloned();
        self.visit_node(node)
    }

    fn check_lit_array(
        &mut self, lit: Literal<AstNode>, ty_hint: Option<Type>,
    ) -> Result<(Literal<AstNode>, Type), String> {
        // Extract the elements vec and the type of the array elements. Will always be None as
        // assigned by the parser as this point.
        let elements = match lit {
            Literal::Array { elements, .. } => elements,
            _ => unreachable!("expected array literal"),
        };

        // Clone the inner type hint
        // XXX: Could ty_hint be None?
        let (ty, size) = match ty_hint.unwrap() {
            Type::Array(ty, sz) => (ty.clone(), sz),
            err => unreachable!("array literal has invalid type hint `{}`", err),
        };

        // Make sure array is big enough
        if elements.len() as u32 as usize > size {
            return Err(format!("Array literal too big in assignment: `{}` > `{}`", elements.len(), size));
        }

        // Check every element and make sure they are uniform
        let mut chkd_elements = Vec::with_capacity(elements.len());
        for el in elements {
            let el_node = self.check_node(el, Some(&ty))?;
            let el_ty = el_node.ty().unwrap_or_default();
            if el_ty != ty.as_ref() {
                return Err(format!("Array literal's element wrong type: `{}` isn't a `{}`", el_node, ty));
            }
            chkd_elements.push(el_node);
        }

        // Rebuild the literal and return the type
        Ok((Literal::Array { elements: chkd_elements, inner_ty: Some(*ty.clone()) }, Type::Array(ty, size)))
    }

    // Helper for variable initializations
    fn check_var_init(
        &mut self, name: &str, init: Option<&AstNode>, antn: &Type, caller: &str,
    ) -> Result<AstNode, String> {
        use Type::*;

        // If init exists, make sure it matches the variable's annotation
        if let Some(init) = init {
            let init_node = self.check_node(init.clone(), Some(antn))?;
            let init_ty = init_node.ty().unwrap_or_default();
            if antn != init_ty {
                return Err(format!(
                    "Types don't match in {}. `{}` annotated with `{}` but initial value is `{}`",
                    caller, name, antn, init_ty
                ));
            }
            Ok(init_node)
        } else {
            Ok(match antn {
                Int8 => init_literal!(Int8, 0),
                Int16 => init_literal!(Int16, 0),
                Int32 => init_literal!(Int32, 0),
                Int64 => init_literal!(Int64, 0),
                UInt8 => init_literal!(UInt8, 0),
                UInt16 => init_literal!(UInt16, 0),
                UInt32 => init_literal!(UInt32, 0),
                UInt64 => init_literal!(UInt64, 0),
                Float => init_literal!(Float, 0.0),
                Double => init_literal!(Double, 0.0),
                Char => init_literal!(Char, 0),
                Bool => init_literal!(Bool, false),
                Array(ty, len) => init_literal!(Array, *ty, *len),
                Void => unreachable!("void type for variable initialization annotation"),
                Comp(_) => todo!(),
            })
        }
    }
}

impl<'a> AstVisitor for TypeChecker<'a> {
    type Node = AstNode;
    type Result = Result<AstNode, String>;

    fn visit_node(&mut self, node: Self::Node) -> Self::Result {
        node.accept(self)
    }

    fn visit_for(&mut self, stmt: ast::For<Self::Node>) -> Self::Result {
        // Insert starting variable
        self.symbol_table.enter_scope();
        self.symbol_table.insert(Symbol::new_var(&stmt.start_name, &stmt.start_antn));

        let start_expr = self.check_var_init(
            &stmt.start_name,
            stmt.start_expr.as_deref(),
            &stmt.start_antn,
            "for statement",
        )?;

        // Ensure the loop cond is always a bool
        let cond_expr = self.check_node(*stmt.cond_expr, None)?;

        if cond_expr.ty().unwrap_or_default() != &Type::Bool {
            return Err("For loop conditional should always be a bool".to_string());
        }

        // Make sure the step type matches the starting variable
        let step_expr = self.check_node(*stmt.step_expr, Some(&stmt.start_antn))?;
        let step_ty = step_expr.ty().unwrap_or_default();
        if step_ty != &stmt.start_antn {
            return Err(format!(
                "Step type mismatch in for statement. Step is `{}` but `{}` is `{}`",
                step_ty, stmt.start_name, stmt.start_antn
            ));
        }

        // Check body
        let body_node = self.check_node(*stmt.body, None)?;

        self.symbol_table.leave_scope();

        Ok(AstNode::new_for(
            stmt.start_name.to_owned(),
            stmt.start_antn.to_owned(),
            Some(start_expr),
            cond_expr,
            step_expr,
            body_node,
        ))
    }

    fn visit_let(&mut self, stmt: ast::Let<Self::Node>) -> Self::Result {
        self.symbol_table.insert(Symbol::new_var(stmt.name.as_str(), &stmt.antn));
        let init_node = self.check_var_init(&stmt.name, stmt.init.as_deref(), &stmt.antn, "let statement")?;
        Ok(AstNode::new_let(stmt.name, stmt.antn, Some(init_node)))
    }

    fn visit_fn(&mut self, stmt: ast::Fn<Self::Node>) -> Self::Result {
        let mut proto = *stmt.proto;
        let fn_entry = match self.symbol_table.get(proto.name()).cloned() {
            Some(sym) => sym,
            None => unreachable!("missing symbol table entry for function: {}", proto.name()),
        };

        // If body is None, this is an extern and no checking is needed
        let body = match stmt.body {
            Some(body) => body,
            None => return Ok(AstNode::new_fn(proto, None)),
        };

        // Creates interstitial scope for the arguments in the function definition
        self.symbol_table.enter_scope();

        // Insert args into the local scope table
        for arg in proto.args() {
            self.symbol_table.insert(Symbol::new_var(&arg.0, &arg.1));
        }

        let body_node = self.check_node(*body, None)?;
        let body_ty = body_node.ty().unwrap_or_default();

        // Make sure these are in sync since there's no `check_proto()`
        if proto.name() == "main" {
            if proto.ret_ty() != &Type::Void {
                return Err(format!(
                    "main()'s return value shouldn't be annotated. Found `{}`",
                    proto.ret_ty()
                ));
            }
            proto.set_ret_ty(Type::Void);
        } else {
            proto.set_ret_ty(fn_entry.ret_ty().to_owned());
        }

        // Make sure function return type and the last statement match. Ignore
        // body type when proto is void.
        if fn_entry.ret_ty() != body_ty && fn_entry.ret_ty() != &Type::Void && proto.name() != "main" {
            return Err(format!(
                "Function `{}` should return type `{}` but last statement is `{}`",
                proto.name(),
                fn_entry.ret_ty(),
                body_ty
            ));
        }

        self.symbol_table.leave_scope();

        Ok(AstNode::new_fn(proto, Some(body_node)))
    }

    fn visit_struct(&mut self, stmt: ast::Struct<Self::Node>) -> Self::Result {
        let mut chkd_fields = vec![];
        for node in stmt.fields {
            chkd_fields.push(self.check_node(node.clone(), None)?);
        }

        let mut chkd_methods = vec![];
        for node in stmt.methods {
            chkd_methods.push(self.check_node(node.clone(), None)?);
        }

        Ok(AstNode::new_struct(stmt.name, chkd_fields, chkd_methods))
    }

    // If there's a type hint (in `self.hint`), use it or fail. If not, use the literal's
    // type. Update `lit` with the result and return the type.
    fn visit_lit(&mut self, expr: ast::Lit<Self::Node>) -> Self::Result {
        use Literal::*;

        // TODO: Clean this up
        let lit = expr.value;
        let (new_lit, lit_ty): (Literal<AstNode>, Type) = match &self.hint {
            Some(hint) => match lit {
                Int8(v) => (Int8(v), Type::Int8),
                Int16(v) => (Int16(v), Type::Int16),
                Int32(v) => (Int32(v), Type::Int32),
                Int64(v) => (Int64(v), Type::Int64),
                UInt8(v) => (UInt8(v), Type::UInt8),
                UInt16(v) => (UInt16(v), Type::UInt16),
                UInt32(v) => (UInt32(v), Type::UInt32),
                UInt64(v) => match hint {
                    Type::Int8 => convert_num!(v, Int8, i8),
                    Type::Int16 => convert_num!(v, Int16, i16),
                    Type::Int32 => convert_num!(v, Int32, i32),
                    Type::Int64 => convert_num!(v, Int64, i64),
                    Type::UInt8 => convert_num!(v, UInt8, u8),
                    Type::UInt16 => convert_num!(v, UInt16, u16),
                    Type::UInt32 => convert_num!(v, UInt32, u32),
                    Type::UInt64 => convert_num!(v, UInt64, u64),
                    float_types!() => return Err("Literal is an integer in a float context".to_string()),
                    Type::Bool => return Err("Literal is an integer in a bool context".to_string()),
                    Type::Char => return Err("Literal is an integer in a char context".to_string()),
                    Type::Array(..) => return Err("Literal is an integer in an array context".to_string()),
                    Type::Void => return Err("Literal is an integer in a void context".to_string()),
                    Type::Comp(_) => return Err("Literal is an integer in a compound context".to_string()),
                },
                Float(v) => match hint {
                    Type::Float => convert_num!(v, Float, f32),
                    Type::Double => convert_num!(v, Double, f64),
                    int_types!() => return Err("Literal is a float in an integer context".to_string()),
                    Type::Bool => return Err("Literal is a float in a bool context".to_string()),
                    Type::Char => return Err("Literal is a float in a char context".to_string()),
                    Type::Array(..) => return Err("Literal is a float in an array context".to_string()),
                    _ => unreachable!("float conversion error"),
                },
                Double(v) => (Double(v), Type::Double),
                Bool(v) => (Bool(v), Type::Bool),
                Char(v) => (Char(v), Type::Char),
                Array { .. } => self.check_lit_array(lit, Some(hint.clone()))?,
            },
            None => match lit {
                Int32(v) => (Int32(v), Type::Int32), // Only used for main's return value
                UInt64(v) => {
                    let v = i32::try_from(v).map_err(|_| "Numeric literal out of range")?;
                    (Int32(v), Type::Int32)
                },
                Float(v) => (Float(v), Type::Float),
                Bool(v) => (Bool(v), Type::Bool),
                Char(v) => (Char(v), Type::Char),
                Array { .. } => self.check_lit_array(lit, None)?,
                x => unreachable!("numeric conversion error for {}", x),
            },
        };

        Ok(AstNode::new_lit(new_lit, Some(lit_ty)))
    }

    fn visit_ident(&mut self, expr: ast::Ident) -> Self::Result {
        let ident_ty = self
            .symbol_table
            .get(&expr.name)
            .ok_or(format!("Unknown variable: `{}`", expr.name))?
            .ty()
            .clone();
        Ok(AstNode::new_ident(expr.name, Some(ident_ty)))
    }

    // TODO: Check overflow on math ops
    fn visit_binop(&mut self, expr: ast::BinOp<Self::Node>) -> Self::Result {
        use Operator::*;

        // Make sure LHS is a var in assignments
        if expr.op == Assign
            && !matches!(
                *expr.lhs,
                AstNode { kind: ast::node::Kind::Ident { .. } } | AstNode { kind: ast::node::Kind::Index { .. } }
            )
        {
            return Err("Expected LHS to be a variable for assignment".to_string());
        }

        // Check if either side is a numeric literal. If so use the other side
        // as a type hint for the literal type.
        let (chkd_lhs, lhs_ty, chkd_rhs, rhs_ty);
        if expr.lhs.is_num_literal() {
            chkd_rhs = self.check_node(*expr.rhs, None)?;
            rhs_ty = chkd_rhs.ty().unwrap_or_default();
            chkd_lhs = self.check_node(*expr.lhs, Some(rhs_ty))?;
            lhs_ty = chkd_lhs.ty().unwrap_or_default();
        } else {
            chkd_lhs = self.check_node(*expr.lhs, None)?;
            lhs_ty = chkd_lhs.ty().unwrap_or_default();
            chkd_rhs = self.check_node(*expr.rhs, Some(lhs_ty))?;
            rhs_ty = chkd_rhs.ty().unwrap_or_default();
        }

        // Both sides must match
        if lhs_ty != rhs_ty {
            return Err(format!("Mismatched types in binop: `{}` != `{}`", lhs_ty, rhs_ty));
        }

        // Check the operand types based on the operator used and set the
        // expression type accordingly
        let ty = match expr.op {
            And | Or => {
                if lhs_ty != &Type::Bool || rhs_ty != &Type::Bool {
                    return Err(format!(
                        "Expected bools on either side of `{}`, got lhs: `{}`, rhs: `{}`",
                        expr.op, lhs_ty, rhs_ty
                    ));
                }
                Type::Bool
            },
            Eq | NotEq => {
                match (&lhs_ty, &rhs_ty) {
                    (
                        numeric_types!() | Type::Bool | Type::Char,
                        numeric_types!() | Type::Bool | Type::Char,
                    ) => (),
                    _ => {
                        return Err(format!(
                            "Invalid type combination found in `{}` operation: (lhs: `{}`, rhs: `{}`)",
                            expr.op, lhs_ty, rhs_ty
                        ))
                    },
                };
                Type::Bool
            },
            Gt | GtEq | Lt | LtEq => {
                match (&lhs_ty, &rhs_ty) {
                    (numeric_types!() | Type::Char, numeric_types!() | Type::Char) => (),
                    _ => {
                        return Err(format!(
                            "Invalid type combination found in `{}` operation: (lhs: `{}`, rhs: `{}`)",
                            expr.op, lhs_ty, rhs_ty
                        ))
                    },
                };
                Type::Bool
            },
            Add | Div | Mul | Pow | Sub | BitAnd | BitXor | BitOr => {
                match (&lhs_ty, &rhs_ty) {
                    (numeric_types!(), numeric_types!()) => (),
                    _ => {
                        return Err(format!(
                            "Invalid type combination found in `{}` operation: (lhs: `{}`, rhs: `{}`)",
                            expr.op, lhs_ty, rhs_ty
                        ))
                    },
                };
                lhs_ty.clone()
            },
            _ => Type::Void,
        };

        Ok(AstNode::new_binop(expr.op, chkd_lhs, chkd_rhs, Some(ty)))
    }

    fn visit_unop(&mut self, expr: ast::UnOp<Self::Node>) -> Self::Result {
        let chkd_rhs = self.check_node(*expr.rhs, None)?;
        let rhs_ty = chkd_rhs.ty().cloned().unwrap_or_default();
        match rhs_ty {
            numeric_types!() => (),
            _ => {
                return Err(format!(
                    "Expected numeric type in unary operation `{}`, got rhs: `{}`",
                    expr.op, rhs_ty
                ))
            },
        }
        Ok(AstNode::new_unop(expr.op, chkd_rhs, Some(rhs_ty)))
    }

    fn visit_call(&mut self, expr: ast::Call<Self::Node>) -> Self::Result {
        // Pull the function for the call from the table
        let fn_entry = self
            .symbol_table
            .get(&expr.name)
            .ok_or(format!("Call to undefined function: `{}`", expr.name))?
            .clone();

        // Pull out the function arg types
        let fe_arg_tys = fn_entry.arg_tys().to_vec();

        // Check arg length
        let fe_args_len = fe_arg_tys.len();
        let args_len = expr.args.len();
        if fe_arg_tys.len() != expr.args.len() {
            return Err(format!(
                "Call to `{}()` takes {} args and {} were given",
                expr.name, fe_args_len, args_len
            ));
        }

        // Check all args and record their types. Use the function entry arg types as type
        // hints.
        let ret_ty = fn_entry.ret_ty();
        let mut chkd_args = Vec::with_capacity(args_len);
        let mut arg_tys = Vec::with_capacity(args_len);
        for (idx, expr) in expr.args.into_iter().enumerate() {
            let chkd_arg = self.check_node(expr, Some(fe_arg_tys[idx]))?;
            arg_tys.push((idx, chkd_arg.ty().unwrap_or_default().clone()));
            chkd_args.push(chkd_arg);
        }

        // Make sure the function args and the call args jive
        fe_arg_tys.iter().zip(arg_tys).try_for_each(|(fa_ty, (idx, ca_ty))| {
            if *fa_ty != &ca_ty {
                Err(format!(
                    "Type mismatch in arg {} of call to `{}()`: `{}` != `{}`",
                    idx + 1,
                    expr.name,
                    fa_ty,
                    ca_ty
                ))
            } else {
                Ok(())
            }
        })?;

        Ok(AstNode::new_call(expr.name, chkd_args, Some(ret_ty.clone())))
    }

    fn visit_cond(&mut self, expr: ast::Cond<Self::Node>) -> Self::Result {
        let chkd_cond = self.check_node(*expr.cond_expr, None)?;
        let cond_ty = chkd_cond.ty().unwrap_or_default();
        if cond_ty != &Type::Bool {
            return Err("Conditional should always be a bool".to_string());
        }

        let chkd_then = self.check_node(*expr.then_block, None)?;
        let then_ty = chkd_then.ty().cloned().unwrap_or_default();

        // Consequent and alternate must match if else exists
        let mut chkd_else = None;
        if let Some(else_block) = expr.else_block {
            let chkd_node = self.check_node(*else_block, Some(&then_ty))?;
            let else_ty = chkd_node.ty().cloned().unwrap_or_default();
            chkd_else = Some(chkd_node);
            if then_ty != else_ty {
                return Err(format!(
                    "Both arms of conditional must be the same type: `then` == `{}`; `else` == `{}`",
                    then_ty, else_ty
                ));
            }
        }

        Ok(AstNode::new_cond(chkd_cond, chkd_then, chkd_else, Some(then_ty)))
    }

    // Check the block expressions. Ensures statements always eval to void.
    fn visit_block(&mut self, expr: ast::Block<Self::Node>) -> Self::Result {
        self.symbol_table.enter_scope();

        // The block type is set to the final node's type
        let mut chkd_list = Vec::with_capacity(expr.list.len());
        let mut list_ty = Type::Void;
        for node in expr.list {
            let chkd_node = self.check_node(node, None)?;
            list_ty = chkd_node.ty().unwrap_or_default().clone();
            chkd_list.push(chkd_node);
        }

        self.symbol_table.leave_scope();

        Ok(AstNode::new_block(chkd_list, Some(list_ty)))
    }

    fn visit_index(&mut self, expr: ast::Index<Self::Node>) -> Self::Result {
        let chkd_binding = self.check_node(*expr.binding, None)?;
        let binding_ty = match chkd_binding.ty().unwrap_or_default() {
            Type::Array(t, _) => *t.clone(),
            t => return Err(format!("Can't index `{}`", t)),
        };

        // TODO: Coerce into int32
        let chkd_idx = self.check_node(*expr.idx, Some(&Type::Int32))?;
        let idx_ty = chkd_idx.ty().unwrap_or_default();
        if !matches!(idx_ty, int_types!()) {
            return Err(format!("Array index must be an `int`, found `{}`", idx_ty));
        } else if !matches!(idx_ty, Type::Int32) {
            return Err("Index must be an int32 (for now)".to_string());
        }

        Ok(AstNode::new_index(chkd_binding, chkd_idx, Some(binding_ty)))
    }
}
