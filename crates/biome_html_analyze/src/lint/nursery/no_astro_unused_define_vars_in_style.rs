use biome_analyze::{
    Ast, Rule, RuleDiagnostic, RuleSource, context::RuleContext, declare_lint_rule,
};
use biome_console::markup;
use biome_html_syntax::{AstroDefineDirective, HtmlElement, HtmlTextExpression};
use biome_languages::HtmlFileSource;
use biome_rowan::{AstNode, AstNodeList, TextRange, TextSize, TokenText};
use biome_rule_options::no_astro_unused_define_vars_in_style::NoAstroUnusedDefineVarsInStyleOptions;

declare_lint_rule! {
    /// Disallow unused shorthand properties in Astro `<style define:vars={...}>` directives.
    ///
    /// Each shorthand property must appear as a CSS custom property in the same style block.
    ///
    /// ## Examples
    ///
    /// ### Invalid
    ///
    /// ```astro,expect_diagnostic
    /// <style define:vars={{ foreground }}>
    ///     p { color: black; }
    /// </style>
    /// ```
    ///
    /// ### Valid
    ///
    /// ```astro
    /// <style define:vars={{ foreground }}>
    ///     p { color: var(--foreground); }
    /// </style>
    /// ```
    ///
    /// Objects containing aliases, computed properties, spreads, or non-ASCII names are ignored.
    /// Style blocks containing CSS escapes are also ignored.
    ///
    /// ## References
    ///
    /// - [Astro `define:vars`](https://docs.astro.build/en/reference/directives-reference/#definevars)
    pub NoAstroUnusedDefineVarsInStyle {
        version: "next",
        name: "noAstroUnusedDefineVarsInStyle",
        language: "html",
        recommended: false,
        sources: &[RuleSource::EslintAstro("no-unused-define-vars-in-style").inspired()],
    }
}

pub struct UnusedDefineVar {
    expression_text: TokenText,
    name_range: TextRange,
    range: TextRange,
}

impl UnusedDefineVar {
    fn name(&self) -> Option<&str> {
        self.expression_text.text().get(
            usize::from(self.name_range.start())..usize::from(self.name_range.end()),
        )
    }
}

impl Rule for NoAstroUnusedDefineVarsInStyle {
    type Query = Ast<AstroDefineDirective>;
    type State = UnusedDefineVar;
    type Signals = Vec<Self::State>;
    type Options = NoAstroUnusedDefineVarsInStyleOptions;

    fn run(ctx: &RuleContext<Self>) -> Self::Signals {
        if !ctx.source_type::<HtmlFileSource>().is_astro() {
            return Vec::new();
        }

        let directive = ctx.query();
        let Ok(value) = directive.value() else {
            return Vec::new();
        };
        let Some(name) = value.name().ok().and_then(|name| name.token_text_trimmed()) else {
            return Vec::new();
        };
        if name.text() != "vars" {
            return Vec::new();
        }

        let Some(element) = directive.syntax().ancestors().find_map(HtmlElement::cast) else {
            return Vec::new();
        };
        if !element.is_supported_style_tag() {
            return Vec::new();
        }

        let Some(expression) = value
            .initializer()
            .and_then(|initializer| initializer.value().ok())
            .and_then(|value| value.as_html_attribute_single_text_expression().cloned())
            .and_then(|value| value.expression().ok())
        else {
            return Vec::new();
        };
        let Some(definitions) = simple_shorthand_definitions(&expression) else {
            return Vec::new();
        };
        let style_text = element
            .children()
            .iter()
            .next()
            .and_then(|child| {
                child
                    .as_any_html_content()?
                    .as_html_embedded_content()
                    .cloned()
            })
            .and_then(|content| content.value_token().ok())
            .map(|token| token.token_text_trimmed());
        if style_text
            .as_ref()
            .is_some_and(|style_text| style_text.text().contains('\\'))
        {
            return Vec::new();
        }

        definitions
            .into_iter()
            .filter(|definition| {
                let Some(name) = definition.name() else {
                    return false;
                };
                !style_text.as_ref().is_some_and(|style_text| {
                    contains_custom_property_name(style_text.text(), name)
                })
            })
            .collect()
    }

    fn diagnostic(_ctx: &RuleContext<Self>, state: &Self::State) -> Option<RuleDiagnostic> {
        let name = state.name()?;
        Some(
            RuleDiagnostic::new(
                rule_category!(),
                state.range,
                markup! {
                    "The "<Emphasis>{name}</Emphasis>" variable is not used in this style block."
                },
            )
            .note(markup! {
                "Unused "<Emphasis>"define:vars"</Emphasis>" properties add an inline custom property that has no effect on the style block."
            })
            .note("Reference the matching CSS custom property or remove the property."),
        )
    }
}

fn simple_shorthand_definitions(
    expression: &HtmlTextExpression,
) -> Option<Vec<UnusedDefineVar>> {
    let token = expression.html_literal_token().ok()?;
    let expression_text = token.token_text_trimmed();
    let text = expression_text.text();
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    let mut offset = 0;
    let mut definitions = Vec::new();

    for part in inner.split(',') {
        let leading_whitespace = part.len() - part.trim_start().len();
        let name = part.trim();
        let start = offset + leading_whitespace;
        offset += part.len() + 1;

        if name.is_empty() {
            continue;
        }
        if !is_ascii_identifier(name) {
            return None;
        }

        let name_start = TextSize::from((start + 1) as u32);
        let name_range = TextRange::at(name_start, TextSize::from(name.len() as u32));
        definitions.push(UnusedDefineVar {
            expression_text: expression_text.clone(),
            name_range,
            range: name_range + token.text_trimmed_range().start(),
        });
    }

    Some(definitions)
}

fn is_ascii_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

fn contains_custom_property_name(style_text: &str, name: &str) -> bool {
    style_text.match_indices("--").any(|(start, _)| {
        let has_left_boundary = style_text
            .get(..start)
            .and_then(|prefix| prefix.chars().next_back())
            .is_none_or(|character| !is_raw_css_identifier_continuation(character));
        if !has_left_boundary {
            return false;
        }

        let Some(suffix) = style_text.get(start + 2..) else {
            return false;
        };
        suffix.strip_prefix(name).is_some_and(|suffix| {
            suffix
                .chars()
                .next()
                .is_none_or(|character| !is_raw_css_identifier_continuation(character))
        })
    })
}

fn is_raw_css_identifier_continuation(character: char) -> bool {
    !character.is_ascii()
        || character.is_ascii_alphanumeric()
        || matches!(character, '-' | '_' | '\\')
}
