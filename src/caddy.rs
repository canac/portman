use crate::dependencies::{DataDir, Environment, Exec, ReadFile, WriteFile};
use crate::matcher::Matcher;
use crate::registry::PortRegistry;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

// Return the path the portman Caddyfile import
fn import_path(deps: &impl DataDir) -> Result<PathBuf> {
    Ok(deps.get_data_dir()?.join("Caddyfile"))
}

// Return the path the gallery www directory
fn gallery_www_path(deps: &impl DataDir) -> Result<PathBuf> {
    Ok(deps.get_data_dir()?.join("gallery_www"))
}

// Return the generated gallery
fn generate_gallery_index(registry: &PortRegistry) -> String {
    let projects = registry
        .iter()
        .map(|(name, project)| {
            let location = project.matcher.as_ref().map(|matcher| {
                format!(
                    "\n          <p class=\"monospace\">{}</p>",
                    match matcher {
                        Matcher::Directory { directory } => directory.to_string_lossy().to_string(),
                        Matcher::GitRepository { repository } => repository.clone(),
                    }
                )
            });
            format!(
                r#"        <a class="project" href="https://{name}.localhost">
          <h2>{name}</h2>
          <p>Port: <strong>{}</strong></p>{}
        </a>"#,
                project.port,
                location.unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    format!(
        r#"
<!DOCTYPE html>
<html lang="en">
  <head>
    <style>
      .container {{
        font-family: Arial, Helvetica, sans-serif;
      }}

      .container h1 {{
        text-align: center;
        margin-bottom: 1em;
      }}

      .gallery {{
        flex-wrap: wrap;
        justify-content: center;
        gap: 2em;
        margin: auto 3em;
        display: flex;
      }}

      .project {{
        width: 22em;
        color: #222;
        background-color: #eee;
        border-radius: 1.5em;
        padding: 1em 2em;
        text-decoration: none;
        overflow: scroll;
      }}

      .project:hover {{
        background-color: #ddd;
      }}

      .project:active {{
        background-color: #ccc;
      }}

      .project h1 {{
        text-align: center;
        border-bottom: 1px solid #444;
        padding-bottom: 0.5em;
      }}

      .monospace {{
        font-family: Courier New, Courier, monospace;
        font-size: 0.8em;
      }}
    </style>
    <meta charset="utf-8" />
    <title>portman Localhost Projects</title>
  </head>
  <body>
    <div class="container">
      <h1>portman projects ({})</h1>
      <div class="gallery">
{}
      </div>
    </div>
  </body>
</html>
"#,
        projects.len(),
        projects.join("\n")
    )
}

// Return the Caddyfile as a string
pub fn generate_caddyfile(deps: &impl DataDir, registry: &PortRegistry) -> Result<String> {
    let caddyfile = registry
        .iter()
        .map(|(name, project)| {
            let action = if project.redirect {
                format!("redir http://localhost:{}", project.port)
            } else {
                format!("reverse_proxy localhost:{}", project.port)
            };
            format!("{name}.localhost {{\n\t{action}\n}}\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "localhost {{\n\tfile_server {{\n\t\troot \"{}\"\n\t}}\n}}\n\n{caddyfile}",
        gallery_www_path(deps)?.to_string_lossy()
    ))
}

// Ensure that the root caddyfile contains the import to the portman caddyfile
// The inner option will be None if no updates are necessary
fn update_import(
    deps: &impl DataDir,
    existing_caddyfile: Option<String>,
) -> Result<Option<String>> {
    let import_statement = format!("import \"{}\"\n", import_path(deps)?.to_string_lossy());
    let existing_caddyfile = existing_caddyfile.unwrap_or_default();
    Ok(if existing_caddyfile.contains(import_statement.as_str()) {
        None
    } else {
        Some(format!("{import_statement}{existing_caddyfile}"))
    })
}

// Reload the caddy service with the provided port registry
pub fn reload(
    deps: &(impl DataDir + Environment + Exec + ReadFile + WriteFile),
    registry: &PortRegistry,
) -> Result<()> {
    // Determine the caddyfile path
    let import_path = import_path(deps)?;
    deps.write_file(&import_path, &generate_caddyfile(deps, registry)?)
        .with_context(|| {
            format!(
                "Failed to write Caddyfile at \"{}\"",
                import_path.to_string_lossy()
            )
        })?;

    // Read the existing caddyfile so that we can update it as necessary
    let caddyfile_path = PathBuf::from(deps.read_var("HOMEBREW_PREFIX")?)
        .join("etc")
        .join("Caddyfile");
    let existing_caddyfile = deps.read_file(&caddyfile_path).with_context(|| {
        format!(
            "Failed to read Caddyfile at \"{}\"",
            caddyfile_path.to_string_lossy()
        )
    })?;
    if let Some(caddyfile_contents) = update_import(deps, existing_caddyfile)? {
        deps.write_file(&caddyfile_path, &caddyfile_contents)
            .with_context(|| {
                format!(
                    "Failed to write Caddyfile at \"{}\"",
                    caddyfile_path.to_string_lossy()
                )
            })?;
    }

    // Update the gallery file
    let gallery_index_path = gallery_www_path(deps)?.join(PathBuf::from("index.html"));
    deps.write_file(
        &gallery_index_path,
        generate_gallery_index(registry).as_str(),
    )
    .with_context(|| {
        format!(
            "Failed to write gallery index file at \"{}\"",
            gallery_index_path.to_string_lossy()
        )
    })?;

    // Reload the caddy config using the new Caddyfile
    let (status, _) = deps.exec(
        std::process::Command::new("caddy")
            .args(["reload", "--adapter", "caddyfile", "--config"])
            .arg(caddyfile_path),
    )?;
    if status.success() {
        Ok(())
    } else {
        bail!(
            "Failed to execute \"caddy reload\", failed with error code {}",
            match status.code() {
                Some(code) => code.to_string(),
                None => String::from("unknown"),
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependencies::mocks::data_dir_mock;
    use crate::registry::tests::get_mocked_registry;

    const GOLDEN_CADDYFILE: &str = "localhost {
	file_server {
		root \"/data/gallery_www\"
	}
}

app1.localhost {
\treverse_proxy localhost:3001
}

app2.localhost {
\treverse_proxy localhost:3002
}

app3.localhost {
\tredir http://localhost:3003
}
";

    #[test]
    fn test_caddyfile() -> Result<()> {
        let registry = get_mocked_registry()?;
        let deps = unimock::mock([data_dir_mock()]);
        assert_eq!(generate_caddyfile(&deps, &registry)?, GOLDEN_CADDYFILE);
        Ok(())
    }

    #[test]
    fn test_update_import_no_existing() -> Result<()> {
        let deps = unimock::mock([data_dir_mock()]);
        assert_eq!(
            update_import(&deps, None)?,
            Some(String::from("import \"/data/Caddyfile\"\n"))
        );
        Ok(())
    }

    #[test]
    fn test_update_import_already_present() -> Result<()> {
        let deps = unimock::mock([data_dir_mock()]);
        assert!(update_import(
            &deps,
            Some(String::from(
                "import \"/data/Caddyfile\"\n# Other content\n"
            ))
        )?
        .is_none());
        Ok(())
    }

    #[test]
    fn test_update_import_prepend() -> Result<()> {
        let deps = unimock::mock([data_dir_mock()]);
        assert_eq!(
            update_import(&deps, Some(String::from("# Suffix\n")))?,
            Some(String::from("import \"/data/Caddyfile\"\n# Suffix\n"))
        );
        Ok(())
    }

    #[test]
    fn test_generate_gallery() -> Result<()> {
        let registry = get_mocked_registry()?;
        let gallery = generate_gallery_index(&registry);
        assert!(gallery.contains("portman projects (3)"));
        assert!(gallery.contains("href=\"https://app1.localhost\""));
        assert!(gallery.contains("<p class=\"monospace\">https://github.com/user/app2.git</p>"));
        assert!(gallery.contains("<p class=\"monospace\">/projects/app3</p>"));
        Ok(())
    }
}
