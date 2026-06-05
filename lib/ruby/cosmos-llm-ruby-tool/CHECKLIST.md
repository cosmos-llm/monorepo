# Ruby Gem Checklist

## Project Setup

- [ ] `mtem create ruby <gem-name>` ran — `devenv.nix`, `devenv.yaml`, `devenv.lock`, `Gemfile` present
- [ ] `.envrc` created (`echo "use devenv" > .envrc && direnv allow`)
- [ ] `bundle install` ran inside devenv shell
- [ ] `gemspec` has `required_ruby_version`, `summary`, `description`, `homepage`, `license`
- [ ] `spec.files` does not use `git ls-files`
- [ ] `VERSION` constant lives alone in `lib/<gem_name>/version.rb`

## Code Standards

- [ ] All public methods have YARD comments with `@param`, `@return`, `@raise`
- [ ] Custom error classes inherit from a gem-namespaced base `Error < StandardError`
- [ ] No `rescue Exception` — catch `StandardError` or a specific subclass
- [ ] No mutable default arguments (`def foo(list = [])`)

## Testing

- [ ] `bundle exec rake test` passes
- [ ] At least one test per public method
- [ ] Error/edge cases covered

## Documentation

- [ ] `yard doc --fail-on-warning` exits 0
- [ ] `README.md` has installation, usage, and API reference link
- [ ] `CHANGELOG.md` has an `## Unreleased` or versioned entry

## Release

- [ ] Version bumped in `lib/<gem_name>/version.rb` (semver)
- [ ] `CHANGELOG.md` updated
- [ ] `gem build *.gemspec` succeeds
- [ ] Contents inspected: `gem contents *.gem`
- [ ] `gem push *.gem` ran
- [ ] Git tag created: `git tag -s v<version>`
