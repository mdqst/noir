use super::Ssa;

use ast::{ParsedBlock, ParsedFunction, ParsedSsa};
use lexer::{Lexer, LexerError};
use noirc_errors::Span;
use noirc_frontend::monomorphization::ast::InlineType;
use token::{Keyword, SpannedToken, Token};

use crate::ssa::{ir::function::RuntimeType, parser::ast::ParsedTerminator};

mod ast;
mod lexer;
mod tests;
mod token;

impl Ssa {
    fn from_str(str: &str) -> Result<Ssa, SsaError> {
        let mut parser = Parser::new(str).map_err(SsaError::ParserError)?;
        let parsed_ssa = parser.parse_ssa().map_err(SsaError::ParserError)?;
        parsed_ssa.into_ssa()
    }
}

#[derive(Debug)]
pub(crate) enum SsaError {
    ParserError(ParserError),
}

type ParseResult<T> = Result<T, ParserError>;

pub(crate) struct Parser<'a> {
    lexer: Lexer<'a>,
    token: SpannedToken,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(source: &'a str) -> ParseResult<Self> {
        let lexer = Lexer::new(source);
        let mut parser = Self { lexer, token: eof_spanned_token() };
        parser.token = parser.read_token_internal()?;
        Ok(parser)
    }

    pub(crate) fn parse_ssa(&mut self) -> ParseResult<ParsedSsa> {
        let mut functions = Vec::new();
        while !self.at(Token::Eof) {
            let function = self.parse_function()?;
            functions.push(function);
        }
        Ok(ParsedSsa { functions })
    }

    fn parse_function(&mut self) -> ParseResult<ParsedFunction> {
        let runtime_type = self.parse_runtime_type()?;
        self.eat_or_error(Token::Keyword(Keyword::Fn))?;

        let external_name = self.eat_ident_or_error()?;
        let internal_name = self.eat_ident_or_error()?;

        self.eat_or_error(Token::LeftBrace)?;

        let blocks = self.parse_blocks()?;

        self.eat_or_error(Token::RightBrace)?;

        Ok(ParsedFunction { runtime_type, external_name, internal_name, blocks })
    }

    fn parse_runtime_type(&mut self) -> ParseResult<RuntimeType> {
        let acir = if self.eat_keyword(Keyword::Acir)? {
            true
        } else if self.eat_keyword(Keyword::Brillig)? {
            false
        } else {
            return self.expected_one_of_tokens(&[
                Token::Keyword(Keyword::Acir),
                Token::Keyword(Keyword::Brillig),
            ]);
        };

        self.eat_or_error(Token::LeftParen)?;
        let inline_type = self.parse_inline_type()?;
        self.eat_or_error(Token::RightParen)?;

        if acir {
            Ok(RuntimeType::Acir(inline_type))
        } else {
            Ok(RuntimeType::Brillig(inline_type))
        }
    }

    fn parse_inline_type(&mut self) -> ParseResult<InlineType> {
        if self.eat_keyword(Keyword::Inline)? {
            Ok(InlineType::Inline)
        } else if self.eat_keyword(Keyword::InlineAlways)? {
            Ok(InlineType::InlineAlways)
        } else if self.eat_keyword(Keyword::Fold)? {
            Ok(InlineType::Fold)
        } else if self.eat_keyword(Keyword::NoPredicates)? {
            Ok(InlineType::NoPredicates)
        } else {
            self.expected_one_of_tokens(&[
                Token::Keyword(Keyword::Inline),
                Token::Keyword(Keyword::InlineAlways),
                Token::Keyword(Keyword::Fold),
                Token::Keyword(Keyword::NoPredicates),
            ])
        }
    }

    fn parse_blocks(&mut self) -> ParseResult<Vec<ParsedBlock>> {
        let mut blocks = Vec::new();
        while !self.at(Token::RightBrace) {
            let block = self.parse_block()?;
            blocks.push(block);
        }
        Ok(blocks)
    }

    fn parse_block(&mut self) -> ParseResult<ParsedBlock> {
        let name = self.eat_ident_or_error()?;
        self.eat_or_error(Token::LeftParen)?;
        self.eat_or_error(Token::RightParen)?;
        self.eat_or_error(Token::Colon)?;

        let instructions = Vec::new();
        let terminator = self.parse_terminator()?;
        Ok(ParsedBlock { name, instructions, terminator })
    }

    fn parse_terminator(&mut self) -> ParseResult<ParsedTerminator> {
        if self.eat_keyword(Keyword::Return)? {
            Ok(ParsedTerminator::Return)
        } else {
            self.expected_instruction_or_terminator()
        }
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> ParseResult<bool> {
        if let Token::Keyword(kw) = self.token.token() {
            if *kw == keyword {
                self.bump()?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    fn eat_ident(&mut self) -> ParseResult<Option<String>> {
        if !matches!(self.token.token(), Token::Ident(..)) {
            return Ok(None);
        }

        let token = self.bump()?;
        match token.into_token() {
            Token::Ident(ident) => Ok(Some(ident)),
            _ => unreachable!(),
        }
    }

    fn eat_ident_or_error(&mut self) -> ParseResult<String> {
        if let Some(ident) = self.eat_ident()? {
            Ok(ident)
        } else {
            self.expected_identifier()
        }
    }

    fn eat(&mut self, token: Token) -> ParseResult<bool> {
        if self.token.token() == &token {
            self.bump()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn eat_or_error(&mut self, token: Token) -> ParseResult<()> {
        if self.eat(token.clone())? {
            Ok(())
        } else {
            self.expected_token(token)
        }
    }

    fn at(&self, token: Token) -> bool {
        self.token.token() == &token
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        self.at(Token::Keyword(keyword))
    }

    fn bump(&mut self) -> ParseResult<SpannedToken> {
        let token = self.read_token_internal()?;
        Ok(std::mem::replace(&mut self.token, token))
    }

    fn read_token_internal(&mut self) -> ParseResult<SpannedToken> {
        self.lexer.next_token().map_err(ParserError::LexerError)
    }

    fn expected_instruction_or_terminator<T>(&mut self) -> ParseResult<T> {
        Err(ParserError::ExpectedInstructionOrTerminator(
            self.token.token().clone(),
            self.token.to_span(),
        ))
    }

    fn expected_identifier<T>(&mut self) -> ParseResult<T> {
        Err(ParserError::ExpectedIdentifier(self.token.token().clone(), self.token.to_span()))
    }

    fn expected_token<T>(&mut self, token: Token) -> ParseResult<T> {
        Err(ParserError::ExpectedToken(token, self.token.token().clone(), self.token.to_span()))
    }

    fn expected_one_of_tokens<T>(&mut self, tokens: &[Token]) -> ParseResult<T> {
        Err(ParserError::ExpectedOneOfTokens(
            tokens.to_vec(),
            self.token.token().clone(),
            self.token.to_span(),
        ))
    }
}

#[derive(Debug)]
pub(crate) enum ParserError {
    LexerError(LexerError),
    ExpectedToken(Token, Token, Span),
    ExpectedOneOfTokens(Vec<Token>, Token, Span),
    ExpectedIdentifier(Token, Span),
    ExpectedInstructionOrTerminator(Token, Span),
}

fn eof_spanned_token() -> SpannedToken {
    SpannedToken::new(Token::Eof, Default::default())
}
