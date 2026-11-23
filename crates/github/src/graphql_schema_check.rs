//! A small GraphQL checker for the tests: every field, argument, type
//! condition and variable type a document names must exist in GitHub's
//! schema. No GraphQL validation crate is in the workspace, and this catches
//! what a hand-written fixture cannot: a misspelled or removed field.
//!
//! It does not check argument value types, required arguments or fragment
//! cycles.

use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Name(String),
    Punctuator(char),
    Spread,
    Text,
    Number,
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let characters = source.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(&character) = characters.get(index) {
        match character {
            _ if character.is_whitespace() || character == ',' || character == '\u{feff}' => {
                index += 1;
            }
            '#' => {
                while characters.get(index).is_some_and(|&next| next != '\n') {
                    index += 1;
                }
            }
            '"' if characters.get(index..index + 3) == Some(&['"', '"', '"']) => {
                index += 3;
                loop {
                    match characters.get(index..index + 3) {
                        Some(['"', '"', '"']) => break,
                        Some(['\\', '"', '"']) => index += 3,
                        Some(_) => index += 1,
                        None => return Err("unterminated block string".to_string()),
                    }
                }
                index += 3;
                tokens.push(Token::Text);
            }
            '"' => {
                index += 1;
                loop {
                    match characters.get(index) {
                        Some('"') => break,
                        Some('\\') => index += 2,
                        Some(_) => index += 1,
                        None => return Err("unterminated string".to_string()),
                    }
                }
                index += 1;
                tokens.push(Token::Text);
            }
            '.' if characters.get(index..index + 3) == Some(&['.', '.', '.']) => {
                index += 3;
                tokens.push(Token::Spread);
            }
            _ if character.is_ascii_alphabetic() || character == '_' => {
                let start = index;
                while characters
                    .get(index)
                    .is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_')
                {
                    index += 1;
                }
                tokens.push(Token::Name(characters[start..index].iter().collect()));
            }
            _ if character.is_ascii_digit() || character == '-' => {
                while characters
                    .get(index)
                    .is_some_and(|next| next.is_ascii_alphanumeric() || "-+.".contains(*next))
                {
                    index += 1;
                }
                tokens.push(Token::Number);
            }
            '{' | '}' | '(' | ')' | '[' | ']' | ':' | '!' | '=' | '@' | '|' | '&' | '$' => {
                index += 1;
                tokens.push(Token::Punctuator(character));
            }
            other => return Err(format!("unexpected character {other:?}")),
        }
    }
    Ok(tokens)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Object,
    Interface,
    Union,
    Enum,
    Scalar,
    Input,
}

struct FieldDefinition {
    type_name: String,
    arguments: HashSet<String>,
}

struct TypeDefinition {
    kind: Kind,
    fields: HashMap<String, FieldDefinition>,
}

pub struct Schema {
    types: HashMap<String, TypeDefinition>,
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(source: &str) -> Result<Self, String> {
        Ok(Self {
            tokens: tokenize(source)?,
            position: 0,
        })
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn peek_is(&self, character: char) -> bool {
        self.peek() == Some(&Token::Punctuator(character))
    }

    fn peek_name_is(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Token::Name(found)) if found == name)
    }

    fn next(&mut self) -> Result<Token, String> {
        let token = self
            .peek()
            .cloned()
            .ok_or_else(|| "unexpected end of document".to_string())?;
        self.position += 1;
        Ok(token)
    }

    fn eat(&mut self, character: char) -> bool {
        let found = self.peek_is(character);
        if found {
            self.position += 1;
        }
        found
    }

    fn expect(&mut self, character: char) -> Result<(), String> {
        match self.next()? {
            Token::Punctuator(found) if found == character => Ok(()),
            other => Err(format!("expected `{character}`, found {other:?}")),
        }
    }

    fn name(&mut self) -> Result<String, String> {
        match self.next()? {
            Token::Name(name) => Ok(name),
            other => Err(format!("expected a name, found {other:?}")),
        }
    }

    fn skip_descriptions(&mut self) {
        while self.peek() == Some(&Token::Text) {
            self.position += 1;
        }
    }

    /// Skips from an opening bracket to its partner, nested ones included.
    fn skip_balanced(&mut self, open: char, close: char) -> Result<(), String> {
        self.expect(open)?;
        let mut depth = 1;
        while depth > 0 {
            match self.next()? {
                Token::Punctuator(found) if found == open => depth += 1,
                Token::Punctuator(found) if found == close => depth -= 1,
                _ => {}
            }
        }
        Ok(())
    }

    fn skip_value(&mut self) -> Result<(), String> {
        if self.peek_is('[') {
            self.skip_balanced('[', ']')
        } else if self.peek_is('{') {
            self.skip_balanced('{', '}')
        } else if self.eat('$') {
            self.name().map(drop)
        } else {
            self.next().map(drop)
        }
    }

    fn skip_directives(&mut self) -> Result<(), String> {
        while self.eat('@') {
            self.name()?;
            if self.peek_is('(') {
                self.skip_balanced('(', ')')?;
            }
        }
        Ok(())
    }

    /// The named type inside `[Type!]!` and the like.
    fn type_reference(&mut self) -> Result<String, String> {
        let name = if self.eat('[') {
            let inner = self.type_reference()?;
            self.expect(']')?;
            inner
        } else {
            self.name()?
        };
        self.eat('!');
        Ok(name)
    }
}

impl Schema {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut parser = Parser::new(source)?;
        let mut types = HashMap::new();
        for scalar in ["String", "Int", "Float", "Boolean", "ID"] {
            types.insert(scalar.to_string(), type_of(Kind::Scalar));
        }
        loop {
            parser.skip_descriptions();
            if parser.peek().is_none() {
                break;
            }
            let keyword = parser.name()?;
            match keyword.as_str() {
                "directive" => {
                    parser.expect('@')?;
                    parser.name()?;
                    if parser.peek_is('(') {
                        parser.skip_balanced('(', ')')?;
                    }
                    if parser.name()? != "on" {
                        return Err("expected `on` in a directive".to_string());
                    }
                    parser.eat('|');
                    parser.name()?;
                    while parser.eat('|') {
                        parser.name()?;
                    }
                }
                "scalar" => {
                    let name = parser.name()?;
                    parser.skip_directives()?;
                    types.insert(name, type_of(Kind::Scalar));
                }
                "enum" => {
                    let name = parser.name()?;
                    parser.skip_directives()?;
                    parser.skip_balanced('{', '}')?;
                    types.insert(name, type_of(Kind::Enum));
                }
                "union" => {
                    let name = parser.name()?;
                    parser.skip_directives()?;
                    parser.expect('=')?;
                    parser.eat('|');
                    parser.name()?;
                    while parser.eat('|') {
                        parser.name()?;
                    }
                    types.insert(name, type_of(Kind::Union));
                }
                "type" | "interface" | "input" => {
                    let kind = match keyword.as_str() {
                        "type" => Kind::Object,
                        "interface" => Kind::Interface,
                        _ => Kind::Input,
                    };
                    let name = parser.name()?;
                    if parser.peek_name_is("implements") {
                        parser.name()?;
                        parser.eat('&');
                        parser.name()?;
                        while parser.eat('&') {
                            parser.name()?;
                        }
                    }
                    parser.skip_directives()?;
                    let fields = if parser.peek_is('{') {
                        parse_field_definitions(&mut parser)?
                    } else {
                        HashMap::new()
                    };
                    types.insert(name, TypeDefinition { kind, fields });
                }
                other => return Err(format!("unsupported schema definition `{other}`")),
            }
        }
        Ok(Self { types })
    }

    fn kind(&self, type_name: &str) -> Option<Kind> {
        self.types.get(type_name).map(|definition| definition.kind)
    }
}

fn type_of(kind: Kind) -> TypeDefinition {
    TypeDefinition {
        kind,
        fields: HashMap::new(),
    }
}

fn parse_field_definitions(
    parser: &mut Parser,
) -> Result<HashMap<String, FieldDefinition>, String> {
    let mut fields = HashMap::new();
    parser.expect('{')?;
    loop {
        parser.skip_descriptions();
        if parser.eat('}') {
            return Ok(fields);
        }
        let name = parser.name()?;
        let mut arguments = HashSet::new();
        if parser.eat('(') {
            loop {
                parser.skip_descriptions();
                if parser.eat(')') {
                    break;
                }
                arguments.insert(parser.name()?);
                parser.expect(':')?;
                parser.type_reference()?;
                if parser.eat('=') {
                    parser.skip_value()?;
                }
                parser.skip_directives()?;
            }
        }
        parser.expect(':')?;
        let type_name = parser.type_reference()?;
        if parser.eat('=') {
            parser.skip_value()?;
        }
        parser.skip_directives()?;
        fields.insert(
            name,
            FieldDefinition {
                type_name,
                arguments,
            },
        );
    }
}

/// Every problem found in `document`, empty when it fits the schema.
pub fn check_document(schema: &Schema, document: &str) -> Vec<String> {
    let mut checker = Checker {
        schema,
        problems: Vec::new(),
        fragments: HashSet::new(),
        spreads: Vec::new(),
    };
    if let Err(problem) = checker.check(document) {
        checker.problems.push(problem);
    }
    for spread in &checker.spreads {
        if !checker.fragments.contains(spread) {
            checker
                .problems
                .push(format!("fragment `{spread}` is not defined"));
        }
    }
    checker.problems
}

struct Checker<'a> {
    schema: &'a Schema,
    problems: Vec<String>,
    fragments: HashSet<String>,
    spreads: Vec<String>,
}

impl Checker<'_> {
    fn check(&mut self, document: &str) -> Result<(), String> {
        let mut parser = Parser::new(document)?;
        while parser.peek().is_some() {
            if parser.peek_is('{') {
                self.selection_set(&mut parser, "Query")?;
                continue;
            }
            let keyword = parser.name()?;
            match keyword.as_str() {
                "query" | "mutation" | "subscription" => {
                    let root = match keyword.as_str() {
                        "query" => "Query",
                        "mutation" => "Mutation",
                        _ => "Subscription",
                    };
                    if matches!(parser.peek(), Some(Token::Name(_))) {
                        parser.name()?;
                    }
                    if parser.eat('(') {
                        while !parser.eat(')') {
                            parser.expect('$')?;
                            let variable = parser.name()?;
                            parser.expect(':')?;
                            let type_name = parser.type_reference()?;
                            if self.schema.kind(&type_name).is_none() {
                                self.problems.push(format!(
                                    "variable `${variable}` has unknown type `{type_name}`"
                                ));
                            }
                            if parser.eat('=') {
                                parser.skip_value()?;
                            }
                            parser.skip_directives()?;
                        }
                    }
                    parser.skip_directives()?;
                    self.selection_set(&mut parser, root)?;
                }
                "fragment" => {
                    let name = parser.name()?;
                    if parser.name()? != "on" {
                        return Err(format!("fragment `{name}` has no type condition"));
                    }
                    let type_name = parser.name()?;
                    self.fragments.insert(name);
                    parser.skip_directives()?;
                    self.typed_selection_set(&mut parser, &type_name)?;
                }
                other => return Err(format!("unexpected `{other}` at the top of a document")),
            }
        }
        Ok(())
    }

    fn typed_selection_set(&mut self, parser: &mut Parser, type_name: &str) -> Result<(), String> {
        if self.schema.kind(type_name).is_none() {
            self.problems
                .push(format!("type `{type_name}` does not exist"));
            return parser.skip_balanced('{', '}');
        }
        self.selection_set(parser, type_name)
    }

    fn selection_set(&mut self, parser: &mut Parser, parent: &str) -> Result<(), String> {
        parser.expect('{')?;
        while !parser.eat('}') {
            if parser.peek() == Some(&Token::Spread) {
                parser.next()?;
                if parser.peek_name_is("on") {
                    parser.name()?;
                    let type_name = parser.name()?;
                    parser.skip_directives()?;
                    self.typed_selection_set(parser, &type_name)?;
                } else if matches!(parser.peek(), Some(Token::Name(_))) {
                    let fragment = parser.name()?;
                    self.spreads.push(fragment);
                    parser.skip_directives()?;
                } else {
                    parser.skip_directives()?;
                    self.selection_set(parser, parent)?;
                }
                continue;
            }
            let mut name = parser.name()?;
            if parser.eat(':') {
                name = parser.name()?;
            }
            self.field(parser, parent, &name)?;
        }
        Ok(())
    }

    fn field(&mut self, parser: &mut Parser, parent: &str, name: &str) -> Result<(), String> {
        let field = self
            .schema
            .types
            .get(parent)
            .and_then(|definition| definition.fields.get(name));
        if name == "__typename" {
            parser.skip_directives()?;
            return Ok(());
        }
        let Some(field) = field else {
            self.problems
                .push(format!("field `{parent}.{name}` does not exist"));
            if parser.peek_is('(') {
                parser.skip_balanced('(', ')')?;
            }
            parser.skip_directives()?;
            if parser.peek_is('{') {
                parser.skip_balanced('{', '}')?;
            }
            return Ok(());
        };
        if parser.eat('(') {
            while !parser.eat(')') {
                let argument = parser.name()?;
                if !field.arguments.contains(&argument) {
                    self.problems.push(format!(
                        "argument `{argument}` of `{parent}.{name}` does not exist"
                    ));
                }
                parser.expect(':')?;
                parser.skip_value()?;
            }
        }
        parser.skip_directives()?;
        let child = field.type_name.clone();
        let composite = matches!(
            self.schema.kind(&child),
            Some(Kind::Object | Kind::Interface | Kind::Union)
        );
        if parser.peek_is('{') {
            if !composite {
                self.problems.push(format!(
                    "`{parent}.{name}` is a leaf and takes no selection"
                ));
                return parser.skip_balanced('{', '}');
            }
            self.selection_set(parser, &child)
        } else {
            if composite {
                self.problems
                    .push(format!("`{parent}.{name}` needs a selection of fields"));
            }
            Ok(())
        }
    }
}
