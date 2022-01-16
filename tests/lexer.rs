use lightc::lexer::Token::*;
use lightc::lexer::*;

#[test]
fn test_lexer_full() {
    let input = "\
extern cos(x)

fn arith(x, y) {
    let result = (x + y) * 4 / 4
    a > b
    result
}

fn main() {
    // Call arith()
    let a = arith(36, 434)
    printf(a)
}
";

    let output = [
        Extern,
        Ident("cos".to_string()),
        OpenParen,
        Ident("x".to_string()),
        CloseParen,
        Fn,
        Ident("arith".to_string()),
        OpenParen,
        Ident("x".to_string()),
        Comma,
        Ident("y".to_string()),
        CloseParen,
        OpenBrace,
        Let,
        Ident("result".to_string()),
        Assign,
        OpenParen,
        Ident("x".to_string()),
        Op('+'),
        Ident("y".to_string()),
        CloseParen,
        Op('*'),
        Int(4.0),
        Op('/'),
        Int(4.0),
        Ident("a".to_string()),
        Op('>'),
        Ident("b".to_string()),
        Ident("result".to_string()),
        CloseBrace,
        Fn,
        Ident("main".to_string()),
        OpenParen,
        CloseParen,
        OpenBrace,
        Let,
        Ident("a".to_string()),
        Assign,
        Ident("arith".to_string()),
        OpenParen,
        Int(36.0),
        Comma,
        Int(434.0),
        CloseParen,
        Ident("printf".to_string()),
        OpenParen,
        Ident("a".to_string()),
        CloseParen,
        CloseBrace,
    ];

    let lexer = Lexer::new(input);
    assert_eq!(lexer.collect::<Result<Vec<_>, _>>().unwrap(), &output);
}

#[test]
fn test_lexer_err_num() {
    let input = "let foo = 1b4";
    assert_eq!(
        Lexer::new(input).collect::<Result<Vec<_>, _>>(),
        Err(LexError::InvalidNum)
    );
}

#[test]
fn test_lexer_multiline_comment() {
    let input = "\
let foo = 14
// line1
// line2
foo
";
    let output = [
        Let,
        Ident("foo".to_string()),
        Assign,
        Int(14.0),
        Ident("foo".to_string()),
    ];
    assert_eq!(
        Lexer::new(input).collect::<Result<Vec<_>, _>>().unwrap(),
        &output
    );
}

#[test]
fn test_lexer_trailing_comment() {
    let input = "\
let foo = 14
// line2";
    let output = [Let, Ident("foo".to_string()), Assign, Int(14.0)];
    assert_eq!(
        Lexer::new(input).collect::<Result<Vec<_>, _>>().unwrap(),
        &output
    );
}

#[test]
fn test_lexer_if_else() {
    let input = "\
if x > 3 {
    print(x)
} else {
    exit()
}
";
    let output = [
        If,
        Ident("x".to_string()),
        Op('>'),
        Int(3.0),
        OpenBrace,
        Ident("print".to_string()),
        OpenParen,
        Ident("x".to_string()),
        CloseParen,
        CloseBrace,
        Else,
        OpenBrace,
        Ident("exit".to_string()),
        OpenParen,
        CloseParen,
        CloseBrace,
    ];
    assert_eq!(
        Lexer::new(input).collect::<Result<Vec<_>, _>>().unwrap(),
        &output
    );
}
