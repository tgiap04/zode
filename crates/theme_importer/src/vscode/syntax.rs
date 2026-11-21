use indexmap::IndexMap;
use serde::Deserialize;
use strum::EnumIter;

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum VsCodeTokenScope {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
pub struct VsCodeTokenColor {
    pub name: Option<String>,
    pub scope: Option<VsCodeTokenScope>,
    pub settings: VsCodeTokenColorSettings,
}

#[derive(Debug, Deserialize)]
pub struct VsCodeTokenColorSettings {
    pub foreground: Option<String>,
    pub background: Option<String>,
    #[serde(rename = "fontStyle")]
    pub font_style: Option<String>,
}

#[derive(Debug, PartialEq, Copy, Clone, EnumIter)]
pub enum ZodeSyntaxToken {
    Attribute,
    Boolean,
    Comment,
    CommentDoc,
    Constant,
    Constructor,
    Embedded,
    Emphasis,
    EmphasisStrong,
    Enum,
    Function,
    Hint,
    Keyword,
    Label,
    LinkText,
    LinkUri,
    Number,
    Operator,
    Predictive,
    Preproc,
    Primary,
    Property,
    Punctuation,
    PunctuationBracket,
    PunctuationDelimiter,
    PunctuationListMarker,
    PunctuationSpecial,
    String,
    StringEscape,
    StringRegex,
    StringSpecial,
    StringSpecialSymbol,
    Tag,
    TextLiteral,
    Title,
    Type,
    Variable,
    VariableSpecial,
    Variant,
}

impl std::fmt::Display for ZodeSyntaxToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ZodeSyntaxToken::Attribute => "attribute",
                ZodeSyntaxToken::Boolean => "boolean",
                ZodeSyntaxToken::Comment => "comment",
                ZodeSyntaxToken::CommentDoc => "comment.doc",
                ZodeSyntaxToken::Constant => "constant",
                ZodeSyntaxToken::Constructor => "constructor",
                ZodeSyntaxToken::Embedded => "embedded",
                ZodeSyntaxToken::Emphasis => "emphasis",
                ZodeSyntaxToken::EmphasisStrong => "emphasis.strong",
                ZodeSyntaxToken::Enum => "enum",
                ZodeSyntaxToken::Function => "function",
                ZodeSyntaxToken::Hint => "hint",
                ZodeSyntaxToken::Keyword => "keyword",
                ZodeSyntaxToken::Label => "label",
                ZodeSyntaxToken::LinkText => "link_text",
                ZodeSyntaxToken::LinkUri => "link_uri",
                ZodeSyntaxToken::Number => "number",
                ZodeSyntaxToken::Operator => "operator",
                ZodeSyntaxToken::Predictive => "predictive",
                ZodeSyntaxToken::Preproc => "preproc",
                ZodeSyntaxToken::Primary => "primary",
                ZodeSyntaxToken::Property => "property",
                ZodeSyntaxToken::Punctuation => "punctuation",
                ZodeSyntaxToken::PunctuationBracket => "punctuation.bracket",
                ZodeSyntaxToken::PunctuationDelimiter => "punctuation.delimiter",
                ZodeSyntaxToken::PunctuationListMarker => "punctuation.list_marker",
                ZodeSyntaxToken::PunctuationSpecial => "punctuation.special",
                ZodeSyntaxToken::String => "string",
                ZodeSyntaxToken::StringEscape => "string.escape",
                ZodeSyntaxToken::StringRegex => "string.regex",
                ZodeSyntaxToken::StringSpecial => "string.special",
                ZodeSyntaxToken::StringSpecialSymbol => "string.special.symbol",
                ZodeSyntaxToken::Tag => "tag",
                ZodeSyntaxToken::TextLiteral => "text.literal",
                ZodeSyntaxToken::Title => "title",
                ZodeSyntaxToken::Type => "type",
                ZodeSyntaxToken::Variable => "variable",
                ZodeSyntaxToken::VariableSpecial => "variable.special",
                ZodeSyntaxToken::Variant => "variant",
            }
        )
    }
}

impl ZodeSyntaxToken {
    pub fn find_best_token_color_match<'a>(
        &self,
        token_colors: &'a [VsCodeTokenColor],
    ) -> Option<&'a VsCodeTokenColor> {
        let mut ranked_matches = IndexMap::new();

        for (ix, token_color) in token_colors.iter().enumerate() {
            if token_color.settings.foreground.is_none() {
                continue;
            }

            let Some(rank) = self.rank_match(token_color) else {
                continue;
            };

            if rank > 0 {
                ranked_matches.insert(ix, rank);
            }
        }

        ranked_matches
            .into_iter()
            .max_by_key(|(_, rank)| *rank)
            .map(|(ix, _)| &token_colors[ix])
    }

    fn rank_match(&self, token_color: &VsCodeTokenColor) -> Option<u32> {
        let candidate_scopes = match token_color.scope.as_ref()? {
            VsCodeTokenScope::One(scope) => vec![scope],
            VsCodeTokenScope::Many(scopes) => scopes.iter().collect(),
        }
        .iter()
        .flat_map(|scope| scope.split(',').map(|s| s.trim()))
        .collect::<Vec<_>>();

        let scopes_to_match = self.to_vscode();
        let number_of_scopes_to_match = scopes_to_match.len();

        let mut matches = 0;

        for (ix, scope) in scopes_to_match.into_iter().enumerate() {
            // Assign each entry a weight that is inversely proportional to its
            // position in the list.
            //
            // Entries towards the front are weighted higher than those towards the end.
            let weight = (number_of_scopes_to_match - ix) as u32;

            if candidate_scopes.contains(&scope) {
                matches += 1 + weight;
            }
        }

        Some(matches)
    }

    pub fn fallbacks(&self) -> &[Self] {
        match self {
            ZodeSyntaxToken::CommentDoc => &[ZodeSyntaxToken::Comment],
            ZodeSyntaxToken::Number => &[ZodeSyntaxToken::Constant],
            ZodeSyntaxToken::VariableSpecial => &[ZodeSyntaxToken::Variable],
            ZodeSyntaxToken::PunctuationBracket
            | ZodeSyntaxToken::PunctuationDelimiter
            | ZodeSyntaxToken::PunctuationListMarker
            | ZodeSyntaxToken::PunctuationSpecial => &[ZodeSyntaxToken::Punctuation],
            ZodeSyntaxToken::StringEscape
            | ZodeSyntaxToken::StringRegex
            | ZodeSyntaxToken::StringSpecial
            | ZodeSyntaxToken::StringSpecialSymbol => &[ZodeSyntaxToken::String],
            _ => &[],
        }
    }

    fn to_vscode(self) -> Vec<&'static str> {
        match self {
            ZodeSyntaxToken::Attribute => vec!["entity.other.attribute-name"],
            ZodeSyntaxToken::Boolean => vec!["constant.language"],
            ZodeSyntaxToken::Comment => vec!["comment"],
            ZodeSyntaxToken::CommentDoc => vec!["comment.block.documentation"],
            ZodeSyntaxToken::Constant => {
                vec!["constant", "constant.language", "constant.character"]
            }
            ZodeSyntaxToken::Constructor => {
                vec![
                    "entity.name.tag",
                    "entity.name.function.definition.special.constructor",
                ]
            }
            ZodeSyntaxToken::Embedded => vec!["meta.embedded"],
            ZodeSyntaxToken::Emphasis => vec!["markup.italic"],
            ZodeSyntaxToken::EmphasisStrong => vec![
                "markup.bold",
                "markup.italic markup.bold",
                "markup.bold markup.italic",
            ],
            ZodeSyntaxToken::Enum => vec!["support.type.enum"],
            ZodeSyntaxToken::Function => vec![
                "entity.function",
                "entity.name.function",
                "variable.function",
            ],
            ZodeSyntaxToken::Hint => vec![],
            ZodeSyntaxToken::Keyword => vec![
                "keyword",
                "keyword.other.fn.rust",
                "keyword.control",
                "keyword.control.fun",
                "keyword.control.class",
                "punctuation.accessor",
                "entity.name.tag",
            ],
            ZodeSyntaxToken::Label => vec![
                "label",
                "entity.name",
                "entity.name.import",
                "entity.name.package",
            ],
            ZodeSyntaxToken::LinkText => vec!["markup.underline.link", "string.other.link"],
            ZodeSyntaxToken::LinkUri => vec!["markup.underline.link", "string.other.link"],
            ZodeSyntaxToken::Number => vec!["constant.numeric", "number"],
            ZodeSyntaxToken::Operator => vec!["operator", "keyword.operator"],
            ZodeSyntaxToken::Predictive => vec![],
            ZodeSyntaxToken::Preproc => vec![
                "preproc",
                "meta.preprocessor",
                "punctuation.definition.preprocessor",
            ],
            ZodeSyntaxToken::Primary => vec![],
            ZodeSyntaxToken::Property => vec![
                "variable.member",
                "support.type.property-name",
                "variable.object.property",
                "variable.other.field",
            ],
            ZodeSyntaxToken::Punctuation => vec![
                "punctuation",
                "punctuation.section",
                "punctuation.accessor",
                "punctuation.separator",
                "punctuation.definition.tag",
            ],
            ZodeSyntaxToken::PunctuationBracket => vec![
                "punctuation.bracket",
                "punctuation.definition.tag.begin",
                "punctuation.definition.tag.end",
            ],
            ZodeSyntaxToken::PunctuationDelimiter => vec![
                "punctuation.delimiter",
                "punctuation.separator",
                "punctuation.terminator",
            ],
            ZodeSyntaxToken::PunctuationListMarker => {
                vec!["markup.list punctuation.definition.list.begin"]
            }
            ZodeSyntaxToken::PunctuationSpecial => vec!["punctuation.special"],
            ZodeSyntaxToken::String => vec!["string"],
            ZodeSyntaxToken::StringEscape => {
                vec!["string.escape", "constant.character", "constant.other"]
            }
            ZodeSyntaxToken::StringRegex => vec!["string.regex"],
            ZodeSyntaxToken::StringSpecial => vec!["string.special", "constant.other.symbol"],
            ZodeSyntaxToken::StringSpecialSymbol => {
                vec!["string.special.symbol", "constant.other.symbol"]
            }
            ZodeSyntaxToken::Tag => vec!["tag", "entity.name.tag", "meta.tag.sgml"],
            ZodeSyntaxToken::TextLiteral => vec!["text.literal", "string"],
            ZodeSyntaxToken::Title => vec!["title", "entity.name"],
            ZodeSyntaxToken::Type => vec![
                "entity.name.type",
                "entity.name.type.primitive",
                "entity.name.type.numeric",
                "keyword.type",
                "support.type",
                "support.type.primitive",
                "support.class",
            ],
            ZodeSyntaxToken::Variable => vec![
                "variable",
                "variable.language",
                "variable.member",
                "variable.parameter",
                "variable.parameter.function-call",
            ],
            ZodeSyntaxToken::VariableSpecial => vec![
                "variable.special",
                "variable.member",
                "variable.annotation",
                "variable.language",
            ],
            ZodeSyntaxToken::Variant => vec!["variant"],
        }
    }
}
