use biome_analyze::{
    Ast, Rule, RuleDiagnostic, RuleDomain, RuleSource, context::RuleContext, declare_lint_rule,
};
use biome_console::markup;
use biome_diagnostics::Severity;
use biome_js_syntax::JsExport;
use biome_languages::JsFileSource;
use biome_rowan::{AstSeparatedList, TextRange};
use biome_rule_options::no_astro_prerender_export_outside_pages::NoAstroPrerenderExportOutsidePagesOptions;
use camino::Utf8Path;

declare_lint_rule! {
    /// Reports `prerender` exports in Astro files outside a `pages` directory.
    ///
    /// Astro only uses `export const prerender = true` and `export const prerender = false`
    /// in pages and endpoints. The same exports in components and other files have no effect.
    ///
    /// ## Examples
    ///
    /// ### Invalid
    ///
    /// In `src/components/Card.astro`:
    ///
    /// ```astro,expect_diagnostic,ignore
    /// ---
    /// export const prerender = true;
    /// ---
    /// ```
    ///
    /// ### Valid
    ///
    /// In `src/pages/index.astro`:
    ///
    /// ```astro,ignore
    /// ---
    /// export const prerender = true;
    /// ---
    /// ```
    ///
    /// ## References
    ///
    /// - [Astro on-demand rendering](https://docs.astro.build/en/guides/on-demand-rendering/)
    pub NoAstroPrerenderExportOutsidePages {
        version: "next",
        name: "noAstroPrerenderExportOutsidePages",
        language: "js",
        sources: &[RuleSource::EslintAstro("no-prerender-export-outside-pages").inspired()],
        recommended: true,
        severity: Severity::Warning,
        domains: &[RuleDomain::Astro],
    }
}

impl Rule for NoAstroPrerenderExportOutsidePages {
    type Query = Ast<JsExport>;
    type State = TextRange;
    type Signals = Option<Self::State>;
    type Options = NoAstroPrerenderExportOutsidePagesOptions;

    fn run(ctx: &RuleContext<Self>) -> Self::Signals {
        if !ctx
            .source_type::<JsFileSource>()
            .as_embedding_kind()
            .is_astro_frontmatter()
            || is_in_pages_directory(ctx.file_path())
        {
            return None;
        }

        let export_clause = ctx.query().export_clause().ok()?;
        let declaration_clause = export_clause
            .as_any_js_declaration_clause()?
            .as_js_variable_declaration_clause()?;
        let declaration = declaration_clause.declaration().ok()?;
        if !declaration.is_const() {
            return None;
        }

        let declarator = declaration.declarators().first()?.ok()?;
        if declarator.variable_annotation().is_some() {
            return None;
        }

        let binding_pattern = declarator.id().ok()?;
        let binding = binding_pattern
            .as_any_js_binding()?
            .as_js_identifier_binding()?;
        let name_token = binding.name_token().ok()?;
        if name_token.text_trimmed() != "prerender" {
            return None;
        }

        declarator
            .initializer()?
            .expression()
            .ok()?
            .as_any_js_literal_expression()?
            .as_js_boolean_literal_expression()?;

        Some(name_token.text_trimmed_range())
    }

    fn diagnostic(_ctx: &RuleContext<Self>, range: &Self::State) -> Option<RuleDiagnostic> {
        Some(
            RuleDiagnostic::new(
                rule_category!(),
                range,
                markup! {
                    "The "<Emphasis>"prerender"</Emphasis>" export has no effect on Astro rendering outside a pages directory."
                },
            )
            .note(markup! {
                "Astro only applies "<Emphasis>"prerender"</Emphasis>" to page routes and endpoints."
            })
            .note(markup! {
                "Move this export to the relevant route or remove it."
            }),
        )
    }
}

fn is_in_pages_directory(path: &Utf8Path) -> bool {
    path.components()
        .any(|component| component.as_str() == "pages")
}

#[cfg(test)]
mod tests {
    use super::is_in_pages_directory;
    use camino::Utf8Path;

    #[test]
    fn detects_pages_path_component() {
        assert!(is_in_pages_directory(Utf8Path::new(
            "src/pages/index.astro"
        )));
        assert!(is_in_pages_directory(Utf8Path::new(
            "app/routes/pages/index.astro"
        )));
        assert!(is_in_pages_directory(Utf8Path::new(
            "/workspace/custom/pages/index.astro"
        )));
        assert!(!is_in_pages_directory(Utf8Path::new(
            "src/pages-demo/index.astro"
        )));
        assert!(!is_in_pages_directory(Utf8Path::new(
            "src/components/pages.astro"
        )));
    }
}
