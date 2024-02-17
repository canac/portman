use crate::dependencies::{DataDir, Environment, Exec, ReadFile, WriteFile};
use crate::registry::Registry;
use anyhow::{bail, Result};
use std::fmt::Write;
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
fn generate_gallery_index(registry: &Registry) -> String {
    let project_count = registry.iter_projects().count();
    let projects = registry
        .iter_projects()
        .fold(String::new(), |mut output, (name, project)| {
            let port = project.port;
            let directory = project
                .directory
                .as_ref()
                .map(|directory| {
                    format!(
                        "\n          <p class=\"monospace\">\"{}\"</p>",
                        directory.display()
                    )
                })
                .unwrap_or_default();
            let _ = write!(
                output,
                r#"
        <a class="project" href="https://{name}.localhost">
          <h2>{name}</h2>
          <p>Port: <strong>{port}</strong></p>{directory}
        </a>"#,
            );
            output
        });
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
      <h1>portman projects ({project_count})</h1>
      <div class="gallery">{projects}
      </div>
    </div>
  </body>
</html>
"#,
    )
}

// Return the Caddyfile as a string
pub fn generate_caddyfile(deps: &impl DataDir, registry: &Registry) -> Result<String> {
    let projects = registry
        .iter_projects()
        .fold(String::new(), |mut output, (name, project)| {
            let _ = write!(
                output,
                "\n{name}.localhost {{\n\treverse_proxy localhost:{}\n}}\n",
                project.port
            );
            if let Some(linked_port) = project.linked_port {
                let _ = write!(
                    output,
                    "\nhttp://localhost:{linked_port} {{\n\treverse_proxy localhost:{}\n}}\n",
                    project.port
                );
            }
            output
        });
    Ok(format!(
        "localhost {{\n\tfile_server {{\n\t\troot \"{}\"\n\t}}\n}}\n{projects}",
        gallery_www_path(deps)?.display()
    ))
}

// Ensure that the root caddyfile contains the import to the portman caddyfile
// The inner option will be None if no updates are necessary
fn update_import(
    deps: &impl DataDir,
    existing_caddyfile: Option<String>,
) -> Result<Option<String>> {
    let import_statement = format!("import \"{}\"\n", import_path(deps)?.display());
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
    registry: &Registry,
) -> Result<()> {
    // Determine the caddyfile path
    let import_path = import_path(deps)?;
    deps.write_file(&import_path, &generate_caddyfile(deps, registry)?)?;

    // Read the existing caddyfile so that we can update it as necessary
    let caddyfile_path = PathBuf::from(deps.read_var("HOMEBREW_PREFIX")?)
        .join("etc")
        .join("Caddyfile");
    let existing_caddyfile = deps.read_file(&caddyfile_path)?;
    if let Some(caddyfile_contents) = update_import(deps, existing_caddyfile)? {
        deps.write_file(&caddyfile_path, &caddyfile_contents)?;
    }

    // Update the gallery file
    let gallery_index_path = gallery_www_path(deps)?.join(PathBuf::from("index.html"));
    deps.write_file(
        &gallery_index_path,
        generate_gallery_index(registry).as_str(),
    )?;

    // Reload the caddy config using the new Caddyfile
    let (status, output) = deps.exec(
        std::process::Command::new("caddy")
            .args(["reload", "--adapter", "caddyfile", "--config"])
            .arg(caddyfile_path.clone()),
        &mut (),
    )?;
    if !status.success() {
        bail!(
            "Failed to execute \"caddy reload --adapter caddyfile --config {}\", failed with error code {} and output:\n{output}",
            caddyfile_path.display(),
            match status.code() {
                Some(code) => code.to_string(),
                None => String::from("unknown"),
            }
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use unimock::Unimock;

    use super::*;
    use crate::mocks::{data_dir_mock, get_mocked_registry};

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

http://localhost:3000 {
\treverse_proxy localhost:3002
}

app3.localhost {
\treverse_proxy localhost:3003
}
";

    #[test]
    fn test_caddyfile() {
        let registry = get_mocked_registry().unwrap();
        let deps = Unimock::new(data_dir_mock());
        assert_eq!(
            generate_caddyfile(&deps, &registry).unwrap(),
            GOLDEN_CADDYFILE
        );
    }

    #[test]
    fn test_update_import_no_existing() {
        let deps = Unimock::new(data_dir_mock());
        assert_eq!(
            update_import(&deps, None).unwrap(),
            Some(String::from("import \"/data/Caddyfile\"\n"))
        );
    }

    #[test]
    fn test_update_import_already_present() {
        let deps = Unimock::new(data_dir_mock());
        assert!(update_import(
            &deps,
            Some(String::from(
                "import \"/data/Caddyfile\"\n# Other content\n"
            ))
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn test_update_import_prepend() {
        let deps = Unimock::new(data_dir_mock());
        assert_eq!(
            update_import(&deps, Some(String::from("# Suffix\n"))).unwrap(),
            Some(String::from("import \"/data/Caddyfile\"\n# Suffix\n"))
        );
    }

    #[test]
    fn test_generate_gallery() {
        let registry = get_mocked_registry().unwrap();
        let gallery = generate_gallery_index(&registry);
        assert!(gallery.contains("portman projects (3)"));
        assert!(gallery.contains("href=\"https://app1.localhost\""));
        assert!(gallery.contains("<p class=\"monospace\">\"/projects/app3\"</p>"));
    }
}
