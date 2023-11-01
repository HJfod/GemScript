
use std::{collections::HashMap, fmt::Display, marker::PhantomData};
use crate::parser::node::{ASTNode, ASTRef};
use crate::parser::ast::token::{Op, self};
use crate::shared::logging::{Message, Level, Note, LoggerRef};
use crate::shared::src::{Src, Span, BUILTIN_SPAN};

macro_rules! parse_op {
    (+)  => { token::Add };
    (-)  => { token::Sub };
    (*)  => { token::Mul };
    (/)  => { token::Div };
    (%)  => { token::Mod };
    (==) => { token::Eq };
    (!=) => { token::Neq };
    (<)  => { token::Lss };
    (<=) => { token::Leq };
    (>)  => { token::Gtr };
    (>=) => { token::Geq };
    (&&) => { token::And };
    (||) => { token::Or };
}

macro_rules! define_ops {
    ($res:ident; $a:ident $op:tt $b:ident -> $r:ident; $($rest:tt)+) => {
        define_ops!($res; $a $op $b -> $r;);
        define_ops!($res; $($rest)+)
    };

    ($res:ident; $a:ident $op:tt $b:ident -> $r:ident;) => {
        $res.entities.push(Entity::new_builtin_binop(
            Ty::$a,
            <parse_op!($op)>::new(BUILTIN_SPAN.clone()).into(),
            Ty::$b,
            Ty::$r
        ));
    };
}

pub trait PathLike<'s, 'n> {
    fn resolve<T: Item<'s, 'n>>(&self, space: &Space<'s, 'n, T>) -> FullPath;
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct FullPath {
    path: Vec<String>,
}

impl FullPath {
    pub fn new<T: Into<Vec<String>>>(path: T) -> Self {
        Self { path: path.into() }
    }

    pub fn ends_with(&self, path: &Path) -> bool {
        self.to_string().ends_with(&path.to_string())
    }
}

impl Display for FullPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("::{}", self.path.join("::")))
    }
}

impl<'s, 'n> PathLike<'s, 'n> for FullPath {
    fn resolve<T: Item<'s, 'n>>(&self, _: &Space<'s, 'n, T>) -> FullPath {
        self.clone()
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Path {
    absolute: bool,
    path: Vec<String>,
}

impl Path {
    pub fn new<T: Into<Vec<String>>>(path: T, absolute: bool) -> Self {
        Self { path: path.into(), absolute }
    }

    pub fn into_full(self) -> FullPath {
        FullPath::new(self.path)
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}{}", 
            if self.absolute { "::" } else { "" },
            self.path.join("::")
        ))
    }
}

impl<'s, 'n> PathLike<'s, 'n> for Path {
    fn resolve<T: Item<'s, 'n>>(&self, space: &Space<'s, 'n, T>) -> FullPath {
        space.resolve(self)
    }
}

pub trait Item<'s, 'n>: Sized {
    fn full_path(&self) -> FullPath;
    fn space<'a>(scope: &'a Scope<'s, 'n>) -> &'a Space<'s, 'n, Self>;
    fn space_mut<'a>(scope: &'a mut Scope<'s, 'n>) -> &'a mut Space<'s, 'n, Self>;
    fn can_access_outside_function(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty<'s, 'n> {
    /// Represents that an error occurred during typechecking. Only produced on 
    /// errors - should **never** appear on a succesful compilation!
    /// Always convertible to all other types in all contexts to avoid further 
    /// typechecking errors
    Invalid,
    /// Represents a type whose value is not known yet, but will be inferred 
    /// later
    Inferred,
    /// Represents that the branch which produced this value will never 
    /// finish execution (for example because it returned to an outer scope)
    /// Always convertible to all other types
    /// No value of type `never` can ever exist
    Never,
    /// An unit type, can only have the value `void`
    Void,
    /// Boolean type
    Bool,
    /// 64-bit integer type
    Int,
    /// 64-bit floating point type
    Float,
    /// UTF-8 string type
    String,
    /// Function type (used for named functions that can't capture mutable 
    /// variables from the outer scope)
    Function {
        params: Vec<(String, Ty<'s, 'n>)>,
        ret_ty: Box<Ty<'s, 'n>>,
        decl: ASTRef<'s, 'n>,
    },
    /// Alias for another type
    Alias {
        name: String,
        ty: Box<Ty<'s, 'n>>,
        decl: ASTRef<'s, 'n>,
    },
}

impl<'s, 'n> Ty<'s, 'n> {
    /// Whether this type is one that can't exist as a value (`unknown`, `invalid`, or `never`)
    pub fn is_unreal(&self) -> bool {
        matches!(self, Ty::Inferred | Ty::Invalid | Ty::Never)
    }

    /// Whether this type is  `never` 
    pub fn is_never(&self) -> bool {
        matches!(self, Ty::Never)
    }

    /// Reduce type into its canonical representation, for example remove aliases
    pub fn reduce(&self) -> &Ty<'s, 'n> {
        match self {
            Self::Alias { name: _, ty, decl: _ } => ty,
            other => other,
        }
    }

    /// Test whether this type is implicitly convertible to another type or 
    /// not
    /// 
    /// In most cases this means equality
    pub fn convertible_to(&self, other: &Ty<'s, 'n>) -> bool {
        self.is_unreal() || other.is_unreal() || *self.reduce() == *other.reduce()
    }

    /// Get the return type of this type
    /// 
    /// Returns the return type if the type is a function type, or itself 
    /// if it's something else
    pub fn return_ty(&self) -> Ty<'s, 'n> {
        match self {
            Self::Function { params: _, ret_ty, decl: _ } => *ret_ty.clone(),
            _ => self.clone(),
        }
    }

    /// Get the corresponding AST declaration that produced this type
    pub fn decl(&self) -> ASTRef<'s, 'n> {
        match self {
            Ty::Invalid => ASTRef::Builtin,
            Ty::Inferred => ASTRef::Builtin,
            Ty::Never => ASTRef::Builtin,
            Ty::Void => ASTRef::Builtin,
            Ty::Bool => ASTRef::Builtin,
            Ty::Int => ASTRef::Builtin,
            Ty::Float => ASTRef::Builtin,
            Ty::String => ASTRef::Builtin,
            Ty::Function { params: _, ret_ty: _, decl } => *decl,
            Ty::Alias { name: _, ty: _, decl } => *decl,
        }
    }

    /// Returns `other` if this type is `invalid` or `unknown`
    pub fn or(self, other: Ty<'s, 'n>) -> Ty<'s, 'n> {
        if self.is_unreal() {
            other
        }
        else {
            self
        }
    }
}

impl Display for Ty<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => f.write_str("invalid"),
            Self::Inferred => f.write_str("unknown"),
            Self::Never => f.write_str("never"),
            Self::Void => f.write_str("void"),
            Self::Bool => f.write_str("bool"),
            Self::Int => f.write_str("int"),
            Self::Float => f.write_str("float"),
            Self::String => f.write_str("string"),
            Self::Function { params, ret_ty, decl: _ } => f.write_fmt(format_args!(
                "fun({}) -> {ret_ty}", params.iter()
                    .map(|(p, t)| format!("{p}: {t}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Self::Alias { name, ty: _, decl: _ } => f.write_str(name),
        }
    }
}

impl<'s, 'n> Item<'s, 'n> for Ty<'s, 'n> {
    fn full_path(&self) -> FullPath {
        #[allow(clippy::match_single_binding)]
        match self {
            builtin => FullPath::new([format!("{builtin}")]),
            // todo more
        }
    }

    fn space<'a>(scope: &'a Scope<'s, 'n>) -> &'a Space<'s, 'n, Self> {
        &scope.types
    }
    fn space_mut<'a>(scope: &'a mut Scope<'s, 'n>) -> &'a mut Space<'s, 'n, Self> {
        &mut scope.types
    }

    fn can_access_outside_function(&self) -> bool {
        true
    }
}

pub struct Entity<'s, 'n> {
    name: FullPath,
    decl: ASTRef<'s, 'n>,
    ty: Ty<'s, 'n>,
    mutable: bool,
}

impl<'s, 'n> Entity<'s, 'n> {
    pub fn new(name: FullPath, decl: ASTRef<'s, 'n>, ty: Ty<'s, 'n>, mutable: bool) -> Self {
        Self { name, decl, ty, mutable }
    }

    pub fn new_builtin_binop(a: Ty<'s, 'n>, op: Op, b: Ty<'s, 'n>, ret: Ty<'s, 'n>) -> Self {
        Self {
            name: get_binop_fun_name(&a, &op, &b),
            decl: ASTRef::Builtin,
            ty: Ty::Function {
                params: vec![
                    ("a".into(), a),
                    ("b".into(), b),
                ],
                ret_ty: ret.into(),
                decl: ASTRef::Builtin,
            },
            mutable: false
        }
    }

    pub fn decl(&self) -> ASTRef<'s, 'n> {
        self.decl
    }

    pub fn ty(&self) -> Ty<'s, 'n> {
        self.ty.clone()
    }
}

impl<'s, 'n> Item<'s, 'n> for Entity<'s, 'n> {
    fn full_path(&self) -> FullPath {
        self.name.clone()
    }

    fn space<'a>(scope: &'a Scope<'s, 'n>) -> &'a Space<'s, 'n, Self> {
        &scope.entities
    }
    fn space_mut<'a>(scope: &'a mut Scope<'s, 'n>) -> &'a mut Space<'s, 'n, Self> {
        &mut scope.entities
    }

    fn can_access_outside_function(&self) -> bool {
        // only const entities defined outside the function scope can be accessed 
        !self.mutable
    }
}

pub struct Space<'s, 'n, T: Item<'s, 'n>> {
    entities: HashMap<FullPath, T>,
    _phantom1: PhantomData<&'s Src>,
    _phantom2: PhantomData<&'n i32>,
}

impl<'s, 'n, T: Item<'s, 'n>> Space<'s, 'n, T> {
    pub fn push(&mut self, entity: T) -> &T {
        let name = entity.full_path().clone();
        self.entities.insert(entity.full_path().clone(), entity);
        self.entities.get(&name).unwrap()
    }

    pub fn find(&self, name: &FullPath) -> Option<&T> {
        self.entities.get(name)
    }

    pub fn resolve(&self, path: &Path) -> FullPath {
        self.entities.iter()
            .find(|(full, _)| full.ends_with(path))
            .map(|p| p.0.clone())
            .unwrap_or(path.clone().into_full())
    }
}

impl<'s, 'n, T: Item<'s, 'n>> Default for Space<'s, 'n, T> {
    fn default() -> Self {
        Self {
            entities: Default::default(),
            _phantom1: Default::default(),
            _phantom2: Default::default(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeLevel {
    Opaque,
    Function,
}

pub struct Scope<'s, 'n> {
    types: Space<'s, 'n, Ty<'s, 'n>>,
    entities: Space<'s, 'n, Entity<'s, 'n>>,
    level: ScopeLevel,
    decl: ASTRef<'s, 'n>,
    return_type: Option<Ty<'s, 'n>>,
    return_type_inferred_from: Option<ASTRef<'s, 'n>>,
    is_returned_to: bool,
    has_encountered_never: bool,
    /// Just stores if unreachable expressions have already been found in this scope
    /// Prevents producing six billion "unreachable expression" from a single let statement
    /// after a return
    unreachable_expression_logged: bool,
}

impl<'s, 'n> Scope<'s, 'n> {
    pub fn new(level: ScopeLevel, decl: ASTRef<'s, 'n>, return_type: Option<Ty<'s, 'n>>) -> Self {
        Self {
            types: Space::default(),
            entities: Space::default(),
            level,
            decl,
            return_type,
            return_type_inferred_from: None,
            is_returned_to: false,
            has_encountered_never: false,
            unreachable_expression_logged: false,
        }
    }

    pub fn new_top() -> Self {
        let mut res = Self::new(ScopeLevel::Opaque, ASTRef::Builtin, None);
        res.types.push(Ty::Void);
        res.types.push(Ty::Bool);
        res.types.push(Ty::Int);
        res.types.push(Ty::Float);
        res.types.push(Ty::String);
        define_ops! {
            res;
            Void == Void -> Bool;

            Int + Int -> Int;
            Int - Int -> Int;
            Int / Int -> Int;
            Int * Int -> Int;
            Int % Int -> Int;
            Int == Int -> Bool;
            Int > Int -> Bool;
            Int < Int -> Bool;

            Float + Float -> Float;
            Float - Float -> Float;
            Float / Float -> Float;
            Float * Float -> Float;
            Float % Float -> Float;
            Float == Float -> Bool;
            Float > Float -> Bool;
            Float < Float -> Bool;

            String == String -> Bool;
            String + String -> String;
            String * Int    -> String;
            
            Bool == Bool -> Bool;

            Bool && Bool -> Bool;
            Bool || Bool -> Bool;
        }
        res
    }

    pub fn decl(&self) -> ASTRef<'s, 'n> {
        self.decl
    }
}

pub enum FindItem<T> {
    Some(T),
    NotAvailable(T),
    None,
}

impl<T> FindItem<T> {
    pub fn option(self) -> Option<T> {
        self.into()
    }
}

#[allow(clippy::from_over_into)]
impl<T> Into<Option<T>> for FindItem<T> {
    fn into(self) -> Option<T> {
        match self {
            FindItem::Some(t) => Some(t),
            _ => None,
        }
    }
}

pub enum FindScope<'s, 'n> {
    ByLevel(ScopeLevel),
    ByDecl(ASTRef<'s, 'n>),
    TopMost,
}

impl<'s, 'n> FindScope<'s, 'n> {
    pub fn matches(&self, scope: &Scope<'s, 'n>) -> bool {
        match self {
            Self::ByLevel(level) => scope.level >= *level,
            Self::ByDecl(decl) => scope.decl == *decl,
            Self::TopMost => true,
        }
    }
}

pub struct TypeVisitor<'s, 'n> {
    logger: LoggerRef<'s>,
    scopes: Vec<Scope<'s, 'n>>,
}

impl<'s, 'n> TypeVisitor<'s, 'n> {
    pub fn new(logger: LoggerRef<'s>) -> Self {
        Self {
            logger,
            scopes: vec![Scope::new_top()],
        }
    }

    fn try_find(&self, a: &Ty<'s, 'n>, op: &Op, b: &Ty<'s, 'n>) -> Option<Ty<'s, 'n>> {
        self.find::<Entity<'s, 'n>, _>(&get_binop_fun_name(a, op, b))
            .option()
            .map(move |e| e.ty.return_ty())
    }

    pub fn binop_ty(&self, a: &Ty<'s, 'n>, op: &Op, b: &Ty<'s, 'n>) -> Option<Ty<'s, 'n>> {
        if let Some(ty) = self.try_find(a, op, b) {
            return Some(ty);
        }
        // if op == Op::Neq {
        //     return self.try_find(a, Op::Eq, b);
        // }
        // if op == Op::Eq {
        //     return self.try_find(a, Op::Neq, b);
        // }
        None
    }

    pub fn last_space<T: Item<'s, 'n>>(&self) -> &Space<'s, 'n, T> {
        T::space(self.scopes.last().unwrap())
    }

    pub fn try_push<T: Item<'s, 'n>>(&mut self, item: T, span: &Span<'s>) -> Option<&T> {
        match self.last_space::<Entity>().find(&item.full_path()) {
            None => {
                Some(T::space_mut(self.scopes.last_mut().unwrap()).push(item))
            }
            Some(e) => {
                self.emit_msg(Message::from_span(
                    Level::Error,
                    format!("Entity '{}' already exists in this scope", item.full_path()),
                    span
                ).note(Note::from_span(
                    "Previous declaration here",
                    e.decl().span()
                )));
                None
            }
        }
    }

    pub fn find<'a, T: Item<'s, 'n>, P: PathLike<'s, 'n>>(&'a self, path: &P) -> FindItem<&'a T> {
        let mut outside_function = false;
        for scope in self.scopes.iter().rev() {
            let space = T::space(scope);
            if let Some(e) = space.find(&path.resolve(space)) {
                return if !outside_function || e.can_access_outside_function() {
                    FindItem::Some(e)
                }
                else {
                    FindItem::NotAvailable(e)
                }
            }
            // can't access mutable entities defined outside a function scope
            if scope.level >= ScopeLevel::Function {
                outside_function = true;
            }
        }
        FindItem::None
    }

    pub fn resolve_new(&self, path: Path) -> FullPath {
        // todo: namespaces
        path.into_full()
    }

    pub fn infer_return_type(&mut self, find: FindScope<'s, 'n>, ty: Ty<'s, 'n>, node: ASTRef<'s, 'n>) {
        match self.scopes.iter_mut().rev().find(|s| find.matches(s)) {
            Some(scope) => {
                if let Some(ref old) = scope.return_type {
                    if !ty.convertible_to(old) {
                        self.logger.lock().unwrap().log_msg(Message::from_span(
                            Level::Error,
                            format!("Expected return type to be '{old}', got '{ty}'"),
                            node.span(),
                        ).note_if(scope.return_type_inferred_from.map(|infer|
                            Note::from_span(
                                "Return type inferred from here",
                                infer.span()
                            )
                        )));
                    }
                }
                else {
                    scope.return_type = Some(ty);
                    scope.return_type_inferred_from = Some(node);
                }
                scope.is_returned_to = true;
            }
            None => {
                self.emit_msg(Message::from_span(
                    Level::Error,
                    "Can not return here",
                    node.span(),
                ));
            }
        }
    }

    /// Push a scope onto the top of the scope stack
    pub fn push_scope(&mut self, level: ScopeLevel, decl: ASTRef<'s, 'n>, return_type: Option<Ty<'s, 'n>>) {
        self.scopes.push(Scope::new(level, decl, return_type));
    }

    /// Pop the topmost scope from the scope stack with a default return type and 
    /// which expression resulted in that
    /// 
    /// If the scope contains an explicit return value (such as `yield value`) then 
    /// the default type does nothing (the default type is things like the final 
    /// expression in a block)
    pub fn pop_scope(&mut self, ty: Ty<'s, 'n>, yielding_expr: ASTRef<'s, 'n>) -> Ty<'s, 'n> {
        let scope = self.scopes.pop().expect("internal error: scope stack was empty");
        let mut ret_ty = if scope.is_returned_to {
            scope.return_type.unwrap()
        }
        else {
            if let Some(ref old) = scope.return_type {
                if !ty.convertible_to(old) {
                    self.logger.lock().unwrap().log_msg(Message::from_span(
                        Level::Error,
                        format!("Expected return type to be '{old}', got '{ty}'"),
                        yielding_expr.span(),
                    ));
                }
            }
            ty
        };

        // if any scope above this one has been returned to, this scope will never finish execution
        // if self.any_upper_scope_returns() {
            // ret_ty = Ty::Never;
        // }
        if scope.has_encountered_never {
            ret_ty = Ty::Never;
        }

        // if ret_ty.is_never() {
        //     self.encountered_never();
        // }
        ret_ty
    }

    fn encountered_never(&mut self) {
        self.scopes.last_mut().unwrap().has_encountered_never = true;
    }

    fn check_if_current_expression_is_unreachable<E: ASTNode<'s> + ?Sized>(&mut self, expr: &E) {
        // if any expression before whatever expression called this function 
        // has returned never, then this experssion is never going to be 
        // executed (by definition of never) 
        let scope = self.scopes.last_mut().unwrap();
        if scope.has_encountered_never && !scope.unreachable_expression_logged {
            scope.unreachable_expression_logged = true;
            self.logger.lock().unwrap().log_msg(Message::from_span(
                Level::Error,
                "Unreachable expression",
                expr.span(),
            ));
        }
    }

    pub fn emit_msg(&self, msg: Message<'s>) {
        self.logger.lock().unwrap().log_msg(msg);
    }

    pub fn set_logger(&mut self, logger: LoggerRef<'s>) {
        self.logger = logger;
    }

    pub fn expect_eq(&self, a: Ty<'s, 'n>, b: Ty<'s, 'n>, span: &Span<'s>) -> Ty<'s, 'n> {
        if !a.convertible_to(&b) {
            self.emit_msg(Message::from_span(
                Level::Error,
                format!("Expected type {b}, got type {a}"),
                span,
            ));
        }
        b.or(a)
    }
}

fn get_unop_fun_name(a: &Ty<'_, '_>, op: &Op) -> FullPath {
    FullPath::new([format!("@unop`{a}{op}`")])
}

fn get_binop_fun_name<'s, 'n>(a: &Ty<'s, 'n>, op: &Op, b: &Ty<'s, 'n>) -> FullPath {
    FullPath::new([format!("@binop`{a}{op}{b}`")])
}
