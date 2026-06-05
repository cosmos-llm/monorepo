# Security Policy

## Supported Versions

We release security updates for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

The Durable Programming team takes security vulnerabilities seriously. We appreciate your efforts to responsibly disclose your findings.

### How to Report

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report security vulnerabilities via email to:

**security@cosmos-llm.com**

### What to Include

To help us better understand and resolve the issue, please include as much of the following information as possible:

- **Type of vulnerability** (e.g., input validation, injection, authentication bypass)
- **Full paths of source file(s)** related to the vulnerability
- **Location of the affected source code** (tag/branch/commit or direct URL)
- **Step-by-step instructions to reproduce** the issue
- **Proof-of-concept or exploit code** (if possible)
- **Impact of the issue**, including how an attacker might exploit it

### Response Timeline

We will acknowledge receipt of your vulnerability report within **48 hours** and will send a more detailed response within **7 days** indicating the next steps in handling your report.

After the initial reply, we will:

1. **Confirm the vulnerability** and determine its severity
2. **Develop a fix** for the vulnerability
3. **Prepare a security advisory** for affected users
4. **Release a patch** as soon as possible
5. **Publicly disclose** the vulnerability after the patch is released

### Coordinated Disclosure

We practice coordinated vulnerability disclosure:

- We will work with you to understand the scope and impact
- We will keep you informed of our progress
- We will credit you in the security advisory (unless you prefer to remain anonymous)
- We will coordinate the public disclosure timing with you

### Security Update Process

When we release a security update:

1. We will release a new version with the fix
2. We will publish a security advisory on GitHub
3. We will update the CHANGELOG with security-related changes
4. We will notify users through our communication channels
5. We will update this SECURITY.md file if needed

## Security Considerations

### Input Validation

This library validates all inputs to prevent common vulnerabilities:

- **Filename Validation**: Prevents path traversal attacks by rejecting path separators
- **Null Byte Prevention**: Rejects filenames containing null bytes
- **Type Validation**: Ensures all inputs are of expected types
- **Metadata Validation**: Validates that metadata is a proper Hash

### Immutable Design

All core objects (Blocks, VirtualFiles, Filesystems) are immutable, which:

- Prevents unintended modifications
- Enhances thread safety
- Reduces attack surface for object manipulation

### Safe Rendering

Renderers properly escape output:

- **XML Renderer**: Escapes special characters (`&`, `<`, `>`, `"`, `'`)
- **JSON Renderer**: Uses Ruby's built-in JSON encoding
- **Custom Renderers**: Should implement proper escaping for their format

### File Content Loading

When using `file_content` to load files from the filesystem:

- Validate file paths before loading
- Be aware that file contents are loaded into memory
- Consider file size limits for production use
- Ensure proper access controls on files being loaded

### Virtual Filesystem

The virtual filesystem is isolated from the actual filesystem:

- Cannot be used to access arbitrary filesystem paths
- Path validation prevents directory traversal
- All file operations are in-memory only

## Best Practices for Users

### Safe Context Building

```ruby
# Good: Validate and sanitize user input
user_input = sanitize_input(params[:content])
context = Cosmos::Llm::Context.build do
  block :user, user_input
end

# Avoid: Loading untrusted file paths
# file_content params[:file_path]  # Don't do this!

# Instead: Validate against allowlist
allowed_files = ['/path/to/safe/file1.rb', '/path/to/safe/file2.rb']
if allowed_files.include?(requested_path)
  file_content requested_path
end
```

### Custom Renderer Security

When creating custom renderers:

```ruby
class MyRenderer
  def self.render(builder)
    # Always escape output appropriately for your format
    output = builder.blocks.map do |block|
      escape_html(block.content.to_s)
    end
    output.join("\n")
  end

  def self.escape_html(str)
    # Implement proper escaping
  end
end
```

### Metadata Security

Be cautious with metadata that might be rendered:

```ruby
# Validate metadata values
metadata = {
  role: validate_role(params[:role]),
  source: sanitize_source(params[:source])
}

block = Cosmos::Llm::Context::Block.new(:system, content, metadata)
```

## Known Limitations

### Memory Usage

- Virtual filesystems are stored in memory
- Large file contents can consume significant memory
- Consider file size limits in production environments

### No Filesystem Access Control

- The library doesn't implement filesystem permissions
- Applications should implement their own access control

### No Content Sanitization

- The library doesn't sanitize block content
- Applications must sanitize user-provided content before creating blocks

## Security Updates

We will announce security updates through:

- GitHub Security Advisories
- RubyGems security metadata
- CHANGELOG.md security section
- Email to registered maintainers

## Contact

For security concerns, contact: **security@cosmos-llm.com**

For general support, contact: **commercial@cosmos-llm.com**

## Attribution

We would like to thank the following people for responsibly disclosing security issues:

- (None yet - be the first!)

## Version History

- **2025-01-XX**: Initial security policy published

---

Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
