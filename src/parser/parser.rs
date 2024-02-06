use chumsky::prelude::*;
use num_bigint::{BigUint, TryFromBigIntError};

use crate::parser::{
    ast::{Ast, Expr, Macro, OpDefinition, Statement},
    tokens::Token,
    types::{Ident, Spanned},
};

fn get_ident() -> impl Parser<Token, Ident, Error = Simple<Token>> {
    select! { Token::Ident(ident) => ident }
}

fn dependency_definition() -> impl Parser<Token, Ast, Error = Simple<Token>> {
    just(Token::Dependency).ignore_then(get_ident().map(Ast::Dependency))
}

fn stack_size() -> impl Parser<Token, u16, Error = Simple<Token>> {
    select! { Token::Number(num) => num }.validate(|num, span, emit| match num.try_into() {
        Ok(lol) => lol,
        Err(err) => {
            let err: TryFromBigIntError<BigUint> = err;
            emit(Simple::custom(
                span,
                format!(
                    "Number {} exceeds max valid stack size specifier (max: {})",
                    err.into_original(),
                    u16::MAX
                ),
            ));
            u16::MAX
        }
    })
}

fn dependency_list(token: Token) -> impl Parser<Token, Vec<Ident>, Error = Simple<Token>> {
    just(token)
        .ignore_then(
            get_ident()
                .separated_by(just(Token::Comma))
                .delimited_by(just(Token::OpenRound), just(Token::CloseRound)),
        )
        .or_not()
        .map(Option::unwrap_or_default)
}

fn op_definition() -> impl Parser<Token, Ast, Error = Simple<Token>> {
    let op_def_head = just(Token::Op)
        .ignore_then(get_ident())
        .then_ignore(just(Token::Assign));

    let stack_io = just(Token::Stack).ignore_then(
        stack_size()
            .then_ignore(just(Token::Comma))
            .then(stack_size())
            .delimited_by(just(Token::OpenRound), just(Token::CloseRound)),
    );

    let reads_writes = dependency_list(Token::Reads).then(dependency_list(Token::Writes));

    op_def_head.then(stack_io).then(reads_writes).map(
        |((name, (stack_in, stack_out)), (reads, writes))| {
            Ast::OpDef(OpDefinition {
                name,
                stack_in,
                stack_out,
                reads,
                writes,
            })
        },
    )
}

fn expression() -> impl Parser<Token, Spanned<Expr>, Error = Simple<Token>> {
    recursive(|expr| {
        let arg_list = expr
            .separated_by(just(Token::Comma))
            .delimited_by(just(Token::OpenRound), just(Token::CloseRound))
            .map(Box::new)
            .map_with_span(Spanned::new);
        let call = get_ident()
            .map_with_span(Spanned::new)
            .then(arg_list)
            .map(|(name, args)| Expr::Call { name, args });
        let num = select! { Token::Number(num) => num }
            .validate(|num, span, emit| {
                if num.bits() <= 256 {
                    num
                } else {
                    emit(Simple::custom(
                        span,
                        format!("Expression constant {:x} larger than 32-bytes", num),
                    ));
                    BigUint::from_bytes_le(&[0xff; 32])
                }
            })
            .map(Expr::Num);

        let var = get_ident().map(Expr::Var);

        call.or(num).or(var).map_with_span(Spanned::new)
    })
}

fn statement() -> impl Parser<Token, Statement, Error = Simple<Token>> {
    // my_var =
    let var_assign = get_ident()
        .then_ignore(just(Token::Assign))
        .or_not()
        .map_with_span(|maybe_var, span| maybe_var.map(|ident| Spanned::new(ident, span)));

    // sstore(caller(), add(sload(caller()), sub(0x34, x)))
    // wow = lmao(x, d)
    var_assign
        .then(expression())
        .map(|(ident, expr)| Statement { ident, expr })
        .validate(|stated, span, emit| {
            if stated.ident.is_none() && !matches!(stated.expr.inner, Expr::Call { .. }) {
                emit(Simple::custom(
                    span,
                    format!("Top-level expression not allowed"),
                ))
            }
            stated
        })
}

fn macro_definition() -> impl Parser<Token, Ast, Error = Simple<Token>> {
    // macro TRANSFER =
    let macro_def_head = just(Token::Macro)
        .ignore_then(get_ident())
        .then_ignore(just(Token::Assign));

    // [a, b, c]
    let stack_parameters = || {
        get_ident()
            .separated_by(just(Token::Comma))
            .delimited_by(just(Token::OpenSquare), just(Token::CloseSquare))
    };

    // [a, b, c] ->
    let stack_in = stack_parameters()
        .then_ignore(just(Token::Arrow))
        .or_not()
        .map(Option::unwrap_or_default);

    // { var1 = op(a, ...) ... sstore(x, y)  }
    let body = statement()
        .repeated()
        .delimited_by(just(Token::OpenCurly), just(Token::CloseCurly));

    // -> [result, nice]
    let stack_out = just(Token::Arrow)
        .ignore_then(stack_parameters())
        .or_not()
        .map(Option::unwrap_or_default);

    macro_def_head
        .then(stack_in)
        .then(body)
        .then(stack_out)
        .map(|(((name, inputs), body), outputs)| {
            Ast::Macro(Macro {
                name,
                inputs,
                outputs,
                body,
            })
        })
}

pub fn parser() -> impl Parser<Token, Vec<Spanned<Ast>>, Error = Simple<Token>> {
    just(Token::Define)
        .ignore_then(
            dependency_definition()
                .or(op_definition())
                .or(macro_definition()),
        )
        .map_with_span(Spanned::new)
        .repeated()
        .then_ignore(end())
}

pub fn parse_tokens(tokens: Vec<Token>) -> (Option<Vec<Spanned<Ast>>>, Vec<Simple<Token>>) {
    parser().parse_recovery(tokens)
}
