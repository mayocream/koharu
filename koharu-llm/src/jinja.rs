use minijinja::Environment;
use minijinja_contrib::pycompat::unknown_method_callback;

pub(crate) fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.add_filter("trim", |s: String| s.trim().to_string());
    env.set_unknown_method_callback(unknown_method_callback);
    env
}

#[cfg(test)]
mod tests {
    use minijinja::context;

    use super::environment;

    #[test]
    fn python_string_methods_render_in_embedded_templates() -> anyhow::Result<()> {
        let env = environment();
        let tmpl = env.template_from_str(
            r#"{%- set content = messages[0].content|trim -%}
{%- if not (content.startswith('<tool_response>') and content.endswith('</tool_response>')) -%}
{{ content.split('</think>')[-1].lstrip('\n').rstrip('\n') }}
{%- endif -%}"#,
        )?;

        let rendered = tmpl.render(context! {
            messages => vec![context! {
                role => "assistant",
                content => "<think>\ninternal\n</think>\nvisible\n",
            }],
        })?;

        assert_eq!(rendered, "visible");
        Ok(())
    }
}
