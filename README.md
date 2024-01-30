# portman

`portman` transforms your local project URLs from http://localhost:8080 and http://localhost:3000 into pretty URLs like https://app.localhost and https://project.localhost.

`portman` has three components:

1. The CLI lets you register projects and assign unique autogenerated ports to each one.
1. The shell integration automatically sets the PORT environment variable when you `cd` into the project's directory.
1. The [caddy](https://caddyserver.com) integration automatically generates a Caddyfile that caddy will use to reverse-proxy your localhost:\* urls to https://\*.localhost URLs.

## Installation

```sh
# Install portman CLI
brew install canac/tap/portman

# Install fish shell integration
echo "portman init fish | source" >> ~/.config/fish/config.fish

# Install and start caddy
brew install caddy
brew services start caddy
```

## Basic usage

```sh
# Create a new project and autogenerate a unique port for it
cd /projects/app
portman create

# Check that the shell integration automatically set the $PORT
echo "Port is $PORT"

# Run the project's dev server how you normally would
npm run dev

# Open the app in the browser
open "https://app.localhost"
```

## Linked ports

In addition assigning unique, autogenerated ports to projects, portman can also link running servers to a specific port and dynamically change which project links to that port while those servers are running.

Suppose that you have a work project that needs to be run at http://localhost:3000 because of OAuth configuration that is outside of your control. Further suppose that you have three git worktrees for that project. That gives you an isolated development space for the feature you're working on, a bug you're fixing, and a co-worker's code you're reviewing locally. Without portman, to switch from running the feature worktree to the bugfix worktree you have to stop the feature worktree server listening on port 3000 and start the bugfix worktree server. To run your co-worker's code, you have to then stop the bugfix worktree server and start the code review worktree server. Stopping and restarting servers like this is tedious, especially when frequently switching between projects. portman provides a way to dynamically change which project port 3000 is linked to without needing to stop and restart servers.

First, create a project for each worktree. At this point you can start any or all of the three servers. Each one will use its unique, autogenerated port so they won't conflict with each other.

```sh
# In terminal tab 1...
cd /projects/worktree-feature
portman create
npm run dev

# In terminal tab 2...
cd /projects/worktree-bugfix
portman create
npm run dev

# In terminal tab 3...
cd /projects/worktree-review
portman create
npm run dev
```

Then, run `portman link` to link a project to a specific port.

```sh
# Link http://localhost:3000 to the worktree-bugfix project
portman link 3000 worktree-bugfix

# Sometime later...

# Link http://localhost:3000 to the worktree-review project
portman link 3000 worktree-review
```

To achieve this, portman sets up a reverse-proxy that sends traffic from http://localhost:3000 to the port that the project is linked to.

You can also omit the project name to link the active project.

```sh
# Link http://localhost:3000 to the worktree-feature project
cd /projects/worktree-feature
portman link 3000
```

Lastly, you can link a project to a port when creating it by passing the `--link` flag.

Projects can only be linked to one port at a time, so adding a new linked port removes the previous linked port.

```sh
portman link 3000 worktree-bugfix

# worktree-bug is linked to port 3001 and nothing is linked to port 3000
portman link 3001 worktree-bugfix
```

```sh
cd /projects/worktree-feature-2
# Create the project and link it to port 3000
portman create --link=3000
```

## Gallery

portman provides a simple web server for graphically viewing all of your projects and some basic information about them. It is available at https://localhost.

## Activation

When you create a project, portman remembers the current working directory and associates it with the project. Later when you `cd` to that directory again, portman activates the project by setting the `$PORT` environment variable to the project's port. Note that the shell integration must be enabled for portman to be able to detect changes to the current directory. During activation portman also sets `$PORTMAN_PROJECT` to the name of the active project and sets `$PORTMAN_LINKED_PORT` to the port linked to the active project if there is one.

To create a project without tying it to a specific directory, use the `--no-activate` flag. The project will not be linked to the current directory and therefore cannot be automatically activated. You must also manually provide a name for the project.

```sh
portman create service --no-activate
echo "Port for service is $(portman get service)"
```

## Project names

portman can usually infer a reasonable name for a project when it is omitted from from `create`. The default project is based on the directory, and portman attempts to normalize it to a valid subdomain by converting it to lowercase, converting all characters other than a-z, 0-9, and dash (-) to dashes, stripping leading and trailing dashes, combining adjacent dashes into a single dash, and truncating it to 63 characters.

```sh
cd /projects/app
# Project name defaults to "app"
portman create
```

Projects that don't auto activate aren't associated with a directory. As a result, the project name cannot be inferred and must be provided manually.

```sh
# Project name is explicitly set to "app"
portman create app --no-activate
```

## Configuration

portman has a few configuration parameters that can be tweaked. Run `portman config show` to locate the default config file location. Run `portman config edit` to open the configuration file with `$EDITOR`. You might want to copy the contents of the [`default_config.toml`](default_config.toml) file as a starting point and then make your desired changes. The config file location can also be changed by setting the `PORTMAN_CONFIG` environment variable.

```sh
PORTMAN_CONFIG=~/portman.toml portman config show
```

The config file is in TOML format. This is the default config:

```toml
ranges = [[3000, 3999]]
reserved = []
```

### `ranges`

`ranges` is an array of two-element `[start, end]` arrays representing the allowed port ranges. The first element is the beginning of the port range, inclusive, and the second element is the end of the port range, inclusive. For example, `[[3000, 3999], [8000, 8099]]` would assign ports from 3000-3999 and 8000-8099.

Defaults to `[[3000, 3999]]` if omitted.

### `reserved`

`reserved` is an array of ports that are reserved and will not be assigned to any project. For example, if you want to assign ports between 3000 and 3999, but port 3277 is used by a something on your machine, set `reserved` to `[3277]` to prevent portman from assigning port 3277 to a project.

Defaults to `[]` if omitted.

## Setting up DNS

Chromium-based browsers automatically resolve the `localhost` tld to 127.0.0.1. To use other browsers or other tools, you may need to configure your DNS to resolve \*.localhost to 127.0.0.1. I use [NextDNS](https://nextdns.io) for ad blocking, and it's trivial to add a rewrite in NextDNS for \*.localhost domains.

## Bonus: Starship integration

To show the active project's port in your [Starship](https://starship.rs) prompt, add this to your `starship.toml`:

```toml
[custom.port]
command = 'if test -n "$PORTMAN_LINKED_PORT"; then echo "$PORT -> $PORTMAN_LINKED_PORT"; else echo "$PORT"; fi'
when = 'test -n "$PORT"'
format = ':[$output]($style) '
shell = ['bash', '--noprofile', '--norc']
```

## CLI API

### `portman -h`, `portman --help`

Prints CLI usage information.

### `portman -V`, `portman --version`

Prints portman version.

### `portman init fish`

Prints the shell configuration command to enable the shell integration. Currently, only fish shell is supported, but other shells would be trivial to add.

### `portman create [project-name] [--no-activate|-A] [--link=$port]`

Creates a new project and assigns it a unique, autogenerated port. If `project-name` is not provided, a default is calculated based on the current directory. `project-name` is required if `--no-activate` is present. If `--no-activate` is present, the project is not associated with a directory and will never be activated by the shell integration. See [project names](#project-names) for more details about default project names. If `--link` is provided the new project is linked to the specific port.

`portman create` is idempotent, i.e. calling it multiple times with the same arguments will create a project the first time and do nothing for future invocations. However, an error will occur if the presence of `--no-activate` or the directory differs from the existing project's configuration.

### `portman get [project-name] [--extended|-e]`

Prints a project's port. `project-name` defaults to the active project. If `--extended` is present, the project's name, directory, and linked port are also printed in addition to the port.

### `portman delete [project]`

Deletes a project. `project-name` defaults to the active project. Its autogenerated port may be assigned to another project in the future.

### `portman cleanup`

Deletes all projects whose directories don't exist anymore.

### `portman reset`

Deletes all projects.

### `portman list`

Lists each project in alphabetical order with its ports, directory, and linked port.

### `portman link $port [project-name]`

Links a project to the specified port. `project-name` defaults to the active project.

### `portman unlink [project-name]`

Removes the linked port from a project. `project-name` defaults to the active project.

### `portman caddyfile`

Prints a valid Caddyfile that reverse-proxies all projects' ports to https://\*.localhost URLs where the subdomain is the project name.

### `portman reload-caddy`

Regenerates the Caddyfile and reloads the caddy config. portman updates the Caddyfile and reloads caddy whenever it makes changes, so this command should only be necessary if something else outside of portman's control is manipulating the Caddyfile or caddy config.

### `portman config show`

Prints the configuration that is currently being used.

### `portman config edit`

Opens the configuration file using the command in the `$EDITOR` environment variable.
