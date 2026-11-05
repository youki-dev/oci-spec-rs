//! Parsing of image references such as `registry.example:5000/org/app:v1@sha256:…`.
//!
//! The specs only define parts of a reference: the repository name and tag in the
//! [distribution-spec] and the digest in the [image-spec]. The full format, the registry
//! host and the Docker Hub defaults follow the Go implementation used by Docker and
//! containerd. Parsing matches these versions:
//!
//! - [distribution/reference] `v0.6.0`,
//! - [go-digest] `v1.0.0`, which distribution/reference uses,
//! - distribution-spec and image-spec `v1.1.1`.
//!
//! [distribution-spec]: https://github.com/opencontainers/distribution-spec/blob/v1.1.1/spec.md#pulling-manifests
//! [image-spec]: https://github.com/opencontainers/image-spec/blob/v1.1.1/descriptor.md#digests
//! [distribution/reference]: https://github.com/distribution/reference/tree/v0.6.0
//! [go-digest]: https://github.com/opencontainers/go-digest/tree/v1.0.0

use std::convert::TryFrom;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// NAME_TOTAL_LENGTH_MAX is the maximum total number of characters in a repository name.
const NAME_TOTAL_LENGTH_MAX: usize = 255;

const DOCKER_HUB_DOMAIN_LEGACY: &str = "index.docker.io";
const DOCKER_HUB_DOMAIN: &str = "docker.io";
const DOCKER_HUB_OFFICIAL_REPO_NAME: &str = "library";
const DEFAULT_TAG: &str = "latest";
const LOCALHOST: &str = "localhost";

/// Reasons that parsing a string as a Reference can fail.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    /// Will be returned if digest is ill-formed
    #[error("invalid checksum digest format")]
    DigestInvalidFormat,
    /// Will be returned if digest does not have a correct length
    #[error("invalid checksum digest length")]
    DigestInvalidLength,
    /// Will be returned for an unknown digest algorithm
    #[error("unsupported digest algorithm")]
    DigestUnsupported,
    /// Will be returned for an uppercase character in repository name
    #[error("repository name must be lowercase")]
    NameContainsUppercase,
    /// Will be returned if a name is empty
    #[error("repository name must have at least one component")]
    NameEmpty,
    /// Will be returned if a name is too long
    #[error("repository name must not be more than {NAME_TOTAL_LENGTH_MAX} characters")]
    NameTooLong,
    /// Will be returned if a reference is ill-formed
    #[error("invalid reference format")]
    ReferenceInvalidFormat,
    /// Will be returned if a tag is ill-formed
    #[error("invalid tag format")]
    TagInvalidFormat,
}

/// Reference provides a general type to represent any way of referencing images within an OCI registry.
///
/// # Examples
///
/// Parsing a tagged image reference:
///
/// ```
/// use oci_spec::distribution::Reference;
///
/// let reference: Reference = "docker.io/library/hello-world:latest".parse().unwrap();
///
/// assert_eq!("docker.io/library/hello-world:latest", reference.whole().as_str());
/// assert_eq!("docker.io", reference.registry());
/// assert_eq!("library/hello-world", reference.repository());
/// assert_eq!(Some("latest"), reference.tag());
/// assert_eq!(None, reference.digest());
/// ```
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Reference {
    registry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mirror_registry: Option<String>,
    repository: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<String>,
}

impl Reference {
    /// Create a Reference with a registry, repository and tag.
    pub fn with_tag(registry: String, repository: String, tag: String) -> Self {
        Self {
            registry,
            mirror_registry: None,
            repository,
            tag: Some(tag),
            digest: None,
        }
    }

    /// Create a Reference with a registry, repository and digest.
    pub fn with_digest(registry: String, repository: String, digest: String) -> Self {
        Self {
            registry,
            mirror_registry: None,
            repository,
            tag: None,
            digest: Some(digest),
        }
    }

    /// Create a new instance of [`Reference`] with a registry, repository, tag and digest.
    ///
    /// This is useful when you need to reference an image by both its semantic version (tag)
    /// and its content-addressable digest for immutability.
    ///
    /// # Examples
    ///
    /// ```
    /// use oci_spec::distribution::Reference;
    ///
    /// let reference = Reference::with_tag_and_digest(
    ///     "docker.io".to_string(),
    ///     "library/nginx".to_string(),
    ///     "1.21".to_string(),
    ///     "sha256:abc123...".to_string(),
    /// );
    /// ```
    pub fn with_tag_and_digest(
        registry: String,
        repository: String,
        tag: String,
        digest: String,
    ) -> Self {
        Self {
            registry,
            mirror_registry: None,
            repository,
            tag: Some(tag),
            digest: Some(digest),
        }
    }

    /// Clone the Reference for the same image with a new digest.
    pub fn clone_with_digest(&self, digest: String) -> Self {
        Self {
            registry: self.registry.clone(),
            mirror_registry: self.mirror_registry.clone(),
            repository: self.repository.clone(),
            tag: None,
            digest: Some(digest),
        }
    }

    /// Set a pull mirror registry for this reference.
    ///
    /// The mirror registry will be used to resolve the image, the original registry
    /// is available via the [`Reference::namespace`] function.
    ///
    /// The original registry will be sent with the `ns` query parameter to the mirror registry.
    /// The `ns` query parameter is currently not part of the stable OCI Distribution Spec yet,
    /// but is being discussed to be added and is already used by some other implementations
    /// (for example containerd). So be aware that this feature might not work with all registries.
    ///
    /// Since this is not part of the stable OCI Distribution Spec yet, this feature is exempt from
    /// semver backwards compatibility guarantees and might change in the future.
    #[doc(hidden)]
    pub fn set_mirror_registry(&mut self, registry: String) {
        self.mirror_registry = Some(registry);
    }

    /// Resolve the registry address of a given `Reference`.
    ///
    /// Some registries, such as docker.io, uses a different address for the actual
    /// registry. This function implements such redirection.
    ///
    /// If a mirror registry is set, it will be used instead of the original registry.
    ///
    /// Like [`Reference::registry`], this returns an empty string if the reference has
    /// no registry.
    pub fn resolve_registry(&self) -> &str {
        match (self.registry(), self.mirror_registry.as_deref()) {
            (_, Some(mirror_registry)) => mirror_registry,
            ("docker.io", None) => "index.docker.io",
            (registry, None) => registry,
        }
    }

    /// Returns the name of the registry.
    ///
    /// This is empty if the first component of the name looks like a host (it contains
    /// a `.`) but is not a valid one, for example in `my_registry.local/app`.
    /// As in distribution/reference (Go), the whole name is then the repository.
    pub fn registry(&self) -> &str {
        &self.registry
    }

    /// Returns the name of the repository.
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// Returns the object's tag, if present.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// Returns the object's digest, if present.
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }

    /// Returns the original registry when pulled via a mirror.
    ///
    /// Since this is not part of the stable OCI Distribution Spec yet, this feature is exempt from
    /// semver backwards compatibility guarantees and might change in the future.
    #[doc(hidden)]
    pub fn namespace(&self) -> Option<&str> {
        if self.mirror_registry.is_some() {
            Some(self.registry())
        } else {
            None
        }
    }

    /// Returns the whole reference.
    pub fn whole(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut not_empty = false;
        if !self.registry().is_empty() {
            write!(f, "{}", self.registry())?;
            not_empty = true;
        }
        if !self.repository().is_empty() {
            if not_empty {
                write!(f, "/")?;
            }
            write!(f, "{}", self.repository())?;
            not_empty = true;
        }
        if let Some(t) = self.tag() {
            if not_empty {
                write!(f, ":")?;
            }
            write!(f, "{t}")?;
            not_empty = true;
        }
        if let Some(d) = self.digest() {
            if not_empty {
                write!(f, "@")?;
            }
            write!(f, "{d}")?;
        }
        Ok(())
    }
}

impl FromStr for Reference {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Reference::try_from(s)
    }
}

impl TryFrom<&str> for Reference {
    type Error = ParseError;

    /// Parses a reference like [`ParseNormalizedNamed`] from distribution/reference (Go):
    /// Docker Hub names get the `docker.io` domain and, for single-component names, the
    /// `library/` prefix. Unlike Go, the tag is also set to `latest` when there is no
    /// tag and no digest.
    ///
    /// [`ParseNormalizedNamed`]: https://github.com/distribution/reference/blob/v0.6.0/normalize.go#L56
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(ParseError::NameEmpty);
        }
        // A bare 64 character lowercase hex string is an image ID, not a repository name:
        // https://github.com/distribution/reference/blob/v0.6.0/normalize.go#L57
        if is_identifier(s) {
            return Err(ParseError::ReferenceInvalidFormat);
        }
        let (domain, remainder) = split_docker_domain(s);
        let remote = remainder.split(':').next().unwrap_or_default();
        if has_uppercase(remote) {
            return Err(ParseError::NameContainsUppercase);
        }
        parse_strict(&format!("{domain}/{remainder}"))
    }
}

impl TryFrom<String> for Reference {
    type Error = ParseError;
    fn try_from(string: String) -> Result<Self, Self::Error> {
        TryFrom::try_from(string.as_str())
    }
}

impl From<Reference> for String {
    fn from(reference: Reference) -> Self {
        reference.whole()
    }
}

/// Parses a reference with the grammar of [`Parse`] from distribution/reference (Go),
/// without the Docker Hub defaults. The tag is set to `latest` when there is no tag and
/// no digest.
///
/// The grammar is below. `[x]` means optional, `[x]*` means zero or more times, and
/// `/…/` is a regular expression.
///
/// ```text
/// reference       := name [ ":" tag ] [ "@" digest ]
/// name            := [domain-and-port "/"] remote-name
/// domain-and-port := host [":" port-number]
/// host            := domain-name | "[" /[a-fA-F0-9:]+/ "]"
/// domain-name     := domain-component ["." domain-component]*
/// domain-component := /([a-zA-Z0-9]|[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9])/
/// port-number     := /[0-9]+/
/// remote-name     := path-component ["/" path-component]*
/// path-component  := alpha-numeric [separator alpha-numeric]*
/// alpha-numeric   := /[a-z0-9]+/
/// separator       := /[_.]|__|[-]+/
/// tag             := /[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}/
/// digest          := algorithm ":" /[0-9a-fA-F]{32,}/
/// algorithm       := /[A-Za-z][A-Za-z0-9]*/ [/[-_+.]/ /[A-Za-z][A-Za-z0-9]*/]*
/// ```
///
/// All character classes are ASCII only. This is the grammar in [regexp.go].
///
/// [`Parse`]: https://github.com/distribution/reference/blob/v0.6.0/reference.go#L186
/// [regexp.go]: https://github.com/distribution/reference/blob/v0.6.0/regexp.go
fn parse_strict(s: &str) -> Result<Reference, ParseError> {
    let Some((name, tag, digest)) = match_reference(s) else {
        if lowercase_matches(s) {
            return Err(ParseError::NameContainsUppercase);
        }
        return Err(ParseError::ReferenceInvalidFormat);
    };

    let (registry, repository) = split_name(name);
    if repository.len() > NAME_TOTAL_LENGTH_MAX {
        return Err(ParseError::NameTooLong);
    }
    if let Some(digest) = digest {
        validate_digest(digest)?;
    }

    let tag = match (tag, digest) {
        (None, None) => Some(DEFAULT_TAG),
        (tag, _) => tag,
    };
    Ok(Reference {
        registry: registry.to_owned(),
        mirror_registry: None,
        repository: repository.to_owned(),
        tag: tag.map(str::to_owned),
        digest: digest.map(str::to_owned),
    })
}

/// Splits a name that matches the grammar into registry and repository.
///
/// As in Go, the domain is optional: the first component is the domain only if it is a
/// valid domain and the rest is a valid path. Otherwise the whole name is the
/// repository, and the registry is empty.
/// <https://github.com/distribution/reference/blob/v0.6.0/reference.go#L200-L207>
fn split_name(name: &str) -> (&str, &str) {
    match name.split_once('/') {
        Some((domain, path)) if is_domain_and_port(domain) && is_remote_name(path) => {
            (domain, path)
        }
        _ => ("", name),
    }
}

/// Checks the input against the reference grammar. If it matches, returns its parts:
/// the name, the tag (if any) and the digest (if any). Returns `None` otherwise.
fn match_reference(s: &str) -> Option<(&str, Option<&str>, Option<&str>)> {
    let (name_tag, digest) = match s.split_once('@') {
        Some((name_tag, digest)) => (name_tag, Some(digest)),
        None => (s, None),
    };
    if digest.is_some_and(|d| !is_digest(d)) {
        return None;
    }
    // A name only contains ':' in its domain, which is always followed by '/'.
    // So a ':' after the last '/' must start the tag.
    let (name, tag) = match name_tag.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => (name, Some(tag)),
        _ => (name_tag, None),
    };
    if tag.is_some_and(|t| !is_tag(t)) || !is_name(name) {
        return None;
    }
    Some((name, tag, digest))
}

/// Returns whether `s` would match the grammar after ASCII lowercasing. Go's [`Parse`]
/// then reports `ErrNameContainsUppercase` instead of a generic format error.
///
/// [`Parse`]: https://github.com/distribution/reference/blob/v0.6.0/reference.go#L192
fn lowercase_matches(s: &str) -> bool {
    has_uppercase(s) && match_reference(&s.to_ascii_lowercase()).is_some()
}

/// Whether `s` contains an ASCII uppercase letter.
///
/// Go compares with [`strings.ToLower`], which also lowercases non-ASCII letters such as
/// `É`. Those are invalid in a reference anyway, so they are not treated as uppercase here.
///
/// [`strings.ToLower`]: https://pkg.go.dev/strings#ToLower
fn has_uppercase(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_uppercase())
}

fn is_name(s: &str) -> bool {
    is_remote_name(s)
        || s.split_once('/')
            .is_some_and(|(domain, path)| is_domain_and_port(domain) && is_remote_name(path))
}

fn is_remote_name(s: &str) -> bool {
    s.split('/').all(is_path_component)
}

fn is_path_component(s: &str) -> bool {
    let b = s.as_bytes();
    let alnum = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let mut i = 0;
    loop {
        let start = i;
        while i < b.len() && alnum(&b[i]) {
            i += 1;
        }
        if i == start {
            return false;
        }
        match b.get(i) {
            None => return true,
            Some(b'.') => i += 1,
            Some(b'_') => i += if b.get(i + 1) == Some(&b'_') { 2 } else { 1 },
            Some(b'-') => {
                while b.get(i) == Some(&b'-') {
                    i += 1;
                }
            }
            Some(_) => return false,
        }
    }
}

fn is_domain_and_port(s: &str) -> bool {
    let port = if let Some(rest) = s.strip_prefix('[') {
        let Some((addr, port)) = rest.split_once(']') else {
            return false;
        };
        if addr.is_empty() || !addr.bytes().all(|c| c.is_ascii_hexdigit() || c == b':') {
            return false;
        }
        port
    } else {
        let (host, port) = s.split_at(s.find(':').unwrap_or(s.len()));
        let valid_component = |c: &str| {
            !c.is_empty()
                && !c.starts_with('-')
                && !c.ends_with('-')
                && c.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        };
        if !host.split('.').all(valid_component) {
            return false;
        }
        port
    };
    port.is_empty()
        || port
            .strip_prefix(':')
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

fn is_tag(s: &str) -> bool {
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && word(b[0])
        && b[1..].iter().all(|&c| word(c) || c == b'.' || c == b'-')
}

fn is_digest(s: &str) -> bool {
    let Some((algorithm, hex)) = s.split_once(':') else {
        return false;
    };
    let valid_component = |c: &str| {
        let b = c.as_bytes();
        !b.is_empty() && b[0].is_ascii_alphabetic() && b.iter().all(u8::is_ascii_alphanumeric)
    };
    algorithm.split(['-', '_', '+', '.']).all(valid_component)
        && hex.len() >= 32
        && hex.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Validates a digest that already matches the digest grammar, like [`Digest.Validate`]
/// from go-digest: the algorithm must be known, and the hex must be lowercase and of the
/// right length.
///
/// [`Digest.Validate`]: https://github.com/opencontainers/go-digest/blob/v1.0.0/digest.go#L103
fn validate_digest(digest: &str) -> Result<(), ParseError> {
    let (algorithm, hex) = digest
        .split_once(':')
        .ok_or(ParseError::DigestInvalidFormat)?;
    let len = match algorithm {
        "sha256" => 64,
        "sha384" => 96,
        "sha512" => 128,
        // go-digest only reports an unknown algorithm as unsupported when it is
        // lowercase: [a-z0-9]+(?:[.+_-][a-z0-9]+)*
        // https://github.com/opencontainers/go-digest/blob/v1.0.0/digest.go#L111
        _ if algorithm.split(['.', '+', '_', '-']).all(|c| {
            !c.is_empty()
                && c.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }) =>
        {
            return Err(ParseError::DigestUnsupported)
        }
        _ => return Err(ParseError::DigestInvalidFormat),
    };
    if hex.len() != len {
        return Err(ParseError::DigestInvalidLength);
    }
    if !hex
        .bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(ParseError::DigestInvalidFormat);
    }
    Ok(())
}

/// Whether `s` is a 64 character lowercase hex string, which is an image ID.
fn is_identifier(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Splits a reference into domain and the rest, with the Docker Hub defaults: uses
/// `docker.io` when there is no domain, maps `index.docker.io` to `docker.io`, and adds
/// `library/` to single-component Docker Hub names.
///
/// This is a Rust rewrite of [`splitDockerDomain`] in distribution/reference (Go).
///
/// [`splitDockerDomain`]: https://github.com/distribution/reference/blob/v0.6.0/normalize.go#L126
fn split_docker_domain(name: &str) -> (&str, String) {
    let (domain, remainder) = match name.split_once('/') {
        None => {
            return (
                DOCKER_HUB_DOMAIN,
                format!("{DOCKER_HUB_OFFICIAL_REPO_NAME}/{name}"),
            )
        }
        Some((DOCKER_HUB_DOMAIN_LEGACY, rest)) => (DOCKER_HUB_DOMAIN, rest),
        Some((domain, rest)) if is_domain_candidate(domain) => (domain, rest),
        Some(_) => (DOCKER_HUB_DOMAIN, name),
    };
    if domain == DOCKER_HUB_DOMAIN && !remainder.contains('/') {
        return (
            domain,
            format!("{DOCKER_HUB_OFFICIAL_REPO_NAME}/{remainder}"),
        );
    }
    (domain, remainder.to_owned())
}

/// Whether the first component of a reference is treated as a domain.
///
/// As in Go's `splitDockerDomain`, Docker Hub names are always lowercase, so a first
/// component with an uppercase letter is assumed to be a host.
fn is_domain_candidate(s: &str) -> bool {
    s == LOCALHOST || s.contains(['.', ':']) || has_uppercase(s)
}

#[cfg(test)]
mod test {
    use super::*;

    /// The rules that the OCI specs (distribution-spec and image-spec `v1.1.1`) give for
    /// the parts of a reference, as anchored regular expressions. The tests compare the
    /// parser with them. The name and tag are the distribution-spec's own regular
    /// expressions; the digest is the image-spec grammar written as one. The specs have no
    /// rule for the registry host, so hosts are not checked here.
    mod spec {
        use regex::Regex;

        /// `<name>` in the [distribution-spec](https://github.com/opencontainers/distribution-spec/blob/v1.1.1/spec.md#pulling-manifests).
        pub const NAME: &str =
            r"^[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*(\/[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*)*$";
        /// A tag in the [distribution-spec](https://github.com/opencontainers/distribution-spec/blob/v1.1.1/spec.md#pulling-manifests).
        pub const TAG: &str = r"^[a-zA-Z0-9_][a-zA-Z0-9._-]{0,127}$";
        /// The digest grammar in the [image-spec](https://github.com/opencontainers/image-spec/blob/v1.1.1/descriptor.md#digests).
        pub const DIGEST: &str = r"^[a-z0-9]+([+._-][a-z0-9]+)*:[a-zA-Z0-9=_-]+$";

        pub fn name(s: &str) -> bool {
            Regex::new(NAME).unwrap().is_match(s)
        }

        pub fn tag(s: &str) -> bool {
            Regex::new(TAG).unwrap().is_match(s)
        }

        /// The digest grammar, plus the encoded format of the
        /// [registered algorithms](https://github.com/opencontainers/image-spec/blob/v1.1.1/descriptor.md#registered-algorithms).
        pub fn digest(s: &str) -> bool {
            let hex = |len: usize, encoded: &str| {
                encoded.len() == len
                    && encoded
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            };
            Regex::new(DIGEST).unwrap().is_match(s)
                && match s.split_once(':') {
                    Some(("sha256", encoded)) => hex(64, encoded),
                    Some(("sha512", encoded)) => hex(128, encoded),
                    _ => true,
                }
        }
    }

    /// The regular expressions of distribution/reference (Go) `v0.6.0`, built from the
    /// same pieces as in its [regexp.go](https://github.com/distribution/reference/blob/v0.6.0/regexp.go#L44-L140). Unlike
    /// the specs, they cover the whole reference, including the registry host. In Go, `\w`
    /// only matches ASCII, so the regexes are built with Unicode turned off.
    mod go {
        use regex::{Regex, RegexBuilder};

        const ALPHANUMERIC: &str = r"[a-z0-9]+";
        const SEPARATOR: &str = r"(?:[._]|__|[-]+)";
        const DOMAIN_NAME_COMPONENT: &str = r"(?:[a-zA-Z0-9]|[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9])";
        const OPTIONAL_PORT: &str = r"(?::[0-9]+)?";
        const TAG: &str = r"[\w][\w.-]{0,127}";
        const DIGEST: &str =
            r"[A-Za-z][A-Za-z0-9]*(?:[-_+.][A-Za-z][A-Za-z0-9]*)*[:][[:xdigit:]]{32,}";
        const IPV6_ADDRESS: &str = r"\[(?:[a-fA-F0-9:]+)\]";

        fn optional(s: &str) -> String {
            format!("(?:{s})?")
        }

        fn any_times(s: &str) -> String {
            format!("(?:{s})*")
        }

        fn capture(s: &str) -> String {
            format!("({s})")
        }

        fn anchored(s: &str) -> String {
            format!("^{s}$")
        }

        /// Compiles an anchored pattern with ASCII-only classes, as in Go.
        fn compile(pattern: &str) -> Regex {
            RegexBuilder::new(&anchored(pattern))
                .unicode(false)
                .build()
                .unwrap()
        }

        fn domain_and_port() -> String {
            let domain_name = DOMAIN_NAME_COMPONENT.to_owned()
                + &any_times(&format!(r"\.{DOMAIN_NAME_COMPONENT}"));
            format!("(?:{domain_name}|{IPV6_ADDRESS}){OPTIONAL_PORT}")
        }

        fn remote_name() -> String {
            let path_component =
                ALPHANUMERIC.to_owned() + &any_times(&format!("{SEPARATOR}{ALPHANUMERIC}"));
            path_component.clone() + &any_times(&format!("/{path_component}"))
        }

        /// `digestPat`, anchored.
        pub fn digest_regexp() -> Regex {
            compile(DIGEST)
        }

        /// `domainAndPort`, anchored.
        pub fn domain_and_port_regexp() -> Regex {
            compile(&domain_and_port())
        }

        /// `anchoredNameRegexp`, which captures the domain and the path of a name.
        pub fn anchored_name_regexp() -> Regex {
            let domain = optional(&(capture(&domain_and_port()) + "/"));
            compile(&(domain + &capture(&remote_name())))
        }

        /// Splits a reference into registry, repository, tag and digest with Go's regexes,
        /// like Go's `Parse`. Returns `None` if Go rejects it.
        pub fn parse(s: &str) -> Option<(&str, &str, Option<&str>, Option<&str>)> {
            let c = reference_regexp().captures(s)?;
            let name = anchored_name_regexp()
                .captures(c.get(1).unwrap().as_str())
                .unwrap();
            Some((
                name.get(1).map_or("", |m| m.as_str()),
                name.get(2).unwrap().as_str(),
                c.get(2).map(|m| m.as_str()),
                c.get(3).map(|m| m.as_str()),
            ))
        }

        /// `referenceRegexp`, which captures the name, tag and digest of a reference.
        pub fn reference_regexp() -> Regex {
            let name = optional(&(domain_and_port() + "/")) + &remote_name();
            let pattern = capture(&name)
                + &optional(&(":".to_owned() + &capture(TAG)))
                + &optional(&("@".to_owned() + &capture(DIGEST)));
            compile(&pattern)
        }
    }

    /// Returns every string that can be made from the characters in `alphabet`, from the
    /// empty string up to `max_len` characters long. For example, `['a', 'b']` with
    /// `max_len` 2 gives "", "a", "b", "aa", "ab", "ba" and "bb".
    fn all_strings(alphabet: &[char], max_len: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut last = vec![String::new()];
        for _ in 0..max_len {
            last = last
                .iter()
                .flat_map(|s| alphabet.iter().map(move |c| format!("{s}{c}")))
                .collect();
            all.extend(last.iter().cloned());
        }
        all
    }

    /// Like `assert_eq!`, but the failure message names the checked part of the result and
    /// says which value was expected.
    #[track_caller]
    fn assert_field<T: PartialEq + std::fmt::Debug>(
        what: &str,
        input: &str,
        expected: T,
        actual: T,
    ) {
        assert!(
            expected == actual,
            "{what} of {input:?}: expected {expected:?}, got {actual:?}"
        );
    }

    /// Checks that `parse` accepts `input`, and returns the result.
    #[track_caller]
    fn assert_parses(
        parse: impl Fn(&str) -> Result<Reference, ParseError>,
        input: &str,
    ) -> Reference {
        parse(input).unwrap_or_else(|e| panic!("{input:?} should parse, but got {e:?}"))
    }

    /// Checks that `parse` rejects `input` with the `expected` error.
    #[track_caller]
    fn assert_rejected(
        parse: impl Fn(&str) -> Result<Reference, ParseError>,
        input: &str,
        expected: ParseError,
    ) {
        match parse(input) {
            Ok(r) => {
                panic!("{input:?} should be rejected with {expected:?}, but it parsed as {r:?}")
            }
            Err(e) => assert!(
                e == expected,
                "{input:?} should be rejected with {expected:?}, but got {e:?}"
            ),
        }
    }

    /// Describes a disagreement between a reference implementation (Go or a spec) and our
    /// code, for example `Go's domainAndPort rejects "a:a", but is_domain_and_port
    /// accepts it`.
    /// Long inputs also get their length, which is hard to see otherwise.
    fn disagree(input: &str, reference: &str, reference_accepts: bool, ours: &str) -> String {
        let verb = |accepts| if accepts { "accepts" } else { "rejects" };
        let length = match input.chars().count() {
            n if n > 40 => format!(" ({n} characters)"),
            _ => String::new(),
        };
        format!(
            "{reference} {} {input:?}{length}, but {ours} {} it",
            verb(reference_accepts),
            verb(!reference_accepts)
        )
    }

    mod parse {
        use super::*;
        use const_format::formatcp;
        use rstest::rstest;

        /// A valid sha256 digest (64 hex characters), for cases where its value does not
        /// matter.
        const SHA256: &str =
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

        /// The invalid part of a rejected reference, for `parse_bad_reference`.
        #[derive(Debug)]
        enum Part {
            Name(String),
            Tag(String),
            Digest(String),
        }

        fn name(s: impl Into<String>) -> Option<Part> {
            Some(Part::Name(s.into()))
        }

        fn tag(s: impl Into<String>) -> Option<Part> {
            Some(Part::Tag(s.into()))
        }

        fn digest(s: impl Into<String>) -> Option<Part> {
            Some(Part::Digest(s.into()))
        }

        /// Parses each reference and checks the result. Each part of the result (repository,
        /// tag and digest) is also checked against the regular expressions in the OCI specs
        /// (the `spec` module), and the whole result against the regular expressions of
        /// distribution/reference (the `go` module), which must split it the same way.
        #[rstest(input, registry, repository, tag, digest, whole,
            case("busybox", "docker.io", "library/busybox", Some("latest"), None, "docker.io/library/busybox:latest"),
            case("test.com:tag", "docker.io", "library/test.com", Some("tag"), None, "docker.io/library/test.com:tag"),
            case("test.com:5000", "docker.io", "library/test.com", Some("5000"), None, "docker.io/library/test.com:5000"),
            case("test.com/repo:tag", "test.com", "repo", Some("tag"), None, "test.com/repo:tag"),
            case("test:5000/repo", "test:5000", "repo", Some("latest"), None, "test:5000/repo:latest"),
            case("test:5000/repo:tag", "test:5000", "repo", Some("tag"), None, "test:5000/repo:tag"),
            case("test:5000/repo@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "test:5000", "repo", None, Some("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "test:5000/repo@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            case("test:5000/repo:tag@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "test:5000", "repo", Some("tag"), Some("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "test:5000/repo:tag@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            case("lowercase:Uppercase", "docker.io", "library/lowercase", Some("Uppercase"), None, "docker.io/library/lowercase:Uppercase"),
            case("sub-dom1.foo.com/bar/baz/quux", "sub-dom1.foo.com", "bar/baz/quux", Some("latest"), None, "sub-dom1.foo.com/bar/baz/quux:latest"),
            case("sub-dom1.foo.com/bar/baz/quux:some-long-tag", "sub-dom1.foo.com", "bar/baz/quux", Some("some-long-tag"), None, "sub-dom1.foo.com/bar/baz/quux:some-long-tag"),
            case("b.gcr.io/test.example.com/my-app:test.example.com", "b.gcr.io", "test.example.com/my-app", Some("test.example.com"), None, "b.gcr.io/test.example.com/my-app:test.example.com"),
            // ☃.com in punycode
            case("xn--n3h.com/myimage:xn--n3h.com", "xn--n3h.com", "myimage", Some("xn--n3h.com"), None, "xn--n3h.com/myimage:xn--n3h.com"),
            // 🐳.com in punycode
            case("xn--7o8h.com/myimage:xn--7o8h.com@sha512:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "xn--7o8h.com", "myimage", Some("xn--7o8h.com"), Some("sha512:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "xn--7o8h.com/myimage:xn--7o8h.com@sha512:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            case("foo_bar.com:8080", "docker.io", "library/foo_bar.com", Some("8080"), None, "docker.io/library/foo_bar.com:8080" ),
            case("foo/foo_bar.com:8080", "docker.io", "foo/foo_bar.com", Some("8080"), None, "docker.io/foo/foo_bar.com:8080"),
            case("opensuse/leap:15.3", "docker.io", "opensuse/leap", Some("15.3"), None, "docker.io/opensuse/leap:15.3"),
            // An uppercase first component is treated as a domain, as in Go, so this is
            // accepted with "Uppercase" as the registry:
            // https://github.com/distribution/reference/blob/v0.6.0/normalize.go#L152-L155
            case("Uppercase/lowercase:tag", "Uppercase", "lowercase", Some("tag"), None, "Uppercase/lowercase:tag"),
            case("localhost/app", "localhost", "app", Some("latest"), None, "localhost/app:latest"),
            case("index.docker.io/busybox", "docker.io", "library/busybox", Some("latest"), None, "docker.io/library/busybox:latest"),
            case("a--b/c__d.e_f", "docker.io", "a--b/c__d.e_f", Some("latest"), None, "docker.io/a--b/c__d.e_f:latest"),
            // As in Go, if the first component is not a valid domain, the whole name is the path:
            // https://github.com/distribution/reference/blob/v0.6.0/reference.go#L200-L207
            case("foo.bar_baz/x", "", "foo.bar_baz/x", Some("latest"), None, "foo.bar_baz/x:latest"),
            // IPv6 registry hosts.
            case("[::1]/repo", "[::1]", "repo", Some("latest"), None, "[::1]/repo:latest"),
            case("[::1]:5000/repo", "[::1]:5000", "repo", Some("latest"), None, "[::1]:5000/repo:latest"),
            case("[2001:db8::1]/repo", "[2001:db8::1]", "repo", Some("latest"), None, "[2001:db8::1]/repo:latest"),
            case("[2001:db8:1:2:3:4:5:6]/repo:tag", "[2001:db8:1:2:3:4:5:6]", "repo", Some("tag"), None, "[2001:db8:1:2:3:4:5:6]/repo:tag"),
            case("[2001:db8::1]:5000/repo", "[2001:db8::1]:5000", "repo", Some("latest"), None, "[2001:db8::1]:5000/repo:latest"),
            case("[2001:db8::1]:5000/repo:tag", "[2001:db8::1]:5000", "repo", Some("tag"), None, "[2001:db8::1]:5000/repo:tag"),
            case("[2001:db8::]:5000/repo", "[2001:db8::]:5000", "repo", Some("latest"), None, "[2001:db8::]:5000/repo:latest"),
            case("[2001:DB8::1]:443/org/app:1.0", "[2001:DB8::1]:443", "org/app", Some("1.0"), None, "[2001:DB8::1]:443/org/app:1.0"),
            case(formatcp!("[2001:db8::1]:5000/repo@{SHA256}"), "[2001:db8::1]:5000", "repo", None, Some(SHA256), formatcp!("[2001:db8::1]:5000/repo@{SHA256}")),
            case(formatcp!("[2001:db8::1]:5000/repo:tag@{SHA256}"), "[2001:db8::1]:5000", "repo", Some("tag"), Some(SHA256), formatcp!("[2001:db8::1]:5000/repo:tag@{SHA256}")),
            // Tags may contain uppercase letters, `_`, `.` and `-`, and may start with `_`.
            case("registry.test/image:A", "registry.test", "image", Some("A"), None, "registry.test/image:A"),
            case("registry.test/image:_", "registry.test", "image", Some("_"), None, "registry.test/image:_"),
            case("registry.test/image:tag.with-dash_", "registry.test", "image", Some("tag.with-dash_"), None, "registry.test/image:tag.with-dash_"),
        )]
        fn parse_good_reference(
            input: &str,
            registry: &str,
            repository: &str,
            tag: Option<&str>,
            digest: Option<&str>,
            whole: &str,
        ) {
            println!("input: {}", input);
            let reference = assert_parses(|s| Reference::try_from(s), input);
            println!("{} -> {:?}", input, reference);
            assert_field("registry", input, registry, reference.registry());
            assert_field("repository", input, repository, reference.repository());
            assert_field("tag", input, tag, reference.tag());
            assert_field("digest", input, digest, reference.digest());
            assert_field("whole()", input, whole, &reference.whole());
            assert!(
                spec::name(reference.repository()),
                "the spec regex rejects the repository {:?} of {input:?}",
                reference.repository()
            );
            if let Some(tag) = reference.tag() {
                assert!(
                    spec::tag(tag),
                    "the spec regex rejects the tag {tag:?} of {input:?}"
                );
            }
            if let Some(digest) = reference.digest() {
                assert!(
                    spec::digest(digest),
                    "the spec rules reject the digest {digest:?} of {input:?}"
                );
            }
            let whole = reference.whole();
            let ours = (
                reference.registry(),
                reference.repository(),
                reference.tag(),
                reference.digest(),
            );
            match go::parse(&whole) {
                None => panic!("{input:?} gives {whole:?}, but Go's referenceRegexp rejects it"),
                Some(go) => assert!(
                    go == ours,
                    "Go splits {whole:?} into (registry, repository, tag, digest) {go:?}, but {input:?} gives {ours:?}"
                ),
            }
        }

        /// Checks that each reference is rejected with the expected error.
        ///
        /// The last column names the invalid part of the reference, and the test checks that
        /// the regular expressions in the OCI specs (the `spec` module) reject that part too.
        /// `None` means the specs have no rule for what fails, such as the host, the length
        /// limit, or a rule that is only in Go.
        #[rstest(input, err, rejected,
            case("", ParseError::NameEmpty, name("")),
            case(":justtag", ParseError::ReferenceInvalidFormat, name("")),
            case("@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", ParseError::ReferenceInvalidFormat, name("")),
            case("repo@sha256:ffffffffffffffffffffffffffffffffff", ParseError::DigestInvalidLength, digest("sha256:ffffffffffffffffffffffffffffffffff")),
            case("validname@invaliddigest:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", ParseError::DigestUnsupported, None),
            case("Uppercase:tag", ParseError::NameContainsUppercase, name("Uppercase")),
            case("test:5000/Uppercase/lowercase:tag", ParseError::NameContainsUppercase, name("Uppercase/lowercase")),
            case(&"a".repeat(256), ParseError::NameTooLong, None),
            case("aa/asdf$$^/aa", ParseError::ReferenceInvalidFormat, name("aa/asdf$$^/aa")),
            // Invalid hosts.
            case("[::1/app", ParseError::ReferenceInvalidFormat, None),
            case("[::g]/app", ParseError::ReferenceInvalidFormat, None),
            case("-a.com/app", ParseError::ReferenceInvalidFormat, None),
            // Unicode domains are not supported.
            case("bücher.example/app", ParseError::ReferenceInvalidFormat, None),
            case("☃.com/myimage", ParseError::ReferenceInvalidFormat, None),
            // A 64 character hex name is an image ID.
            case(&"f".repeat(64), ParseError::ReferenceInvalidFormat, None),
            // go-digest requires lowercase hex: https://github.com/opencontainers/go-digest/blob/v1.0.0/algorithm.go#L58-L62
            case(&format!("repo@sha256:{}", "F".repeat(64)), ParseError::DigestInvalidFormat, digest(format!("sha256:{}", "F".repeat(64)))),
            // Only sha256, sha384 and sha512 are supported.
            case("repo@md5:ffffffffffffffffffffffffffffffff", ParseError::DigestUnsupported, None),
            // Go checks for uppercase up to the first ':', which includes "@Sha256" here:
            // https://github.com/distribution/reference/blob/v0.6.0/normalize.go#L62-L69
            case(&format!("repo@Sha256:{}", "f".repeat(64)), ParseError::NameContainsUppercase, digest(format!("Sha256:{}", "f".repeat(64)))),
            // The same with a domain: "@SHA256" is still before the first ':'.
            case(&format!("docker.io/repo@SHA256:{}", "f".repeat(64)), ParseError::NameContainsUppercase, digest(format!("SHA256:{}", "f".repeat(64)))),
            // Here the first ':' comes before the digest, so the uppercase check passes and
            // the digest is rejected instead.
            case(&format!("repo:tag@Sha256:{}", "f".repeat(64)), ParseError::DigestInvalidFormat, digest(format!("Sha256:{}", "f".repeat(64)))),
            // A sha512-length value for sha256.
            case(&format!("repo@sha256:{}", "f".repeat(128)), ParseError::DigestInvalidLength, digest(format!("sha256:{}", "f".repeat(128)))),
            // go-digest checks the length for each algorithm, including sha384, which the
            // image-spec does not register.
            case(&format!("repo@sha384:{}", "f".repeat(64)), ParseError::DigestInvalidLength, None),
            case(&format!("repo@sha512:{}", "f".repeat(130)), ParseError::DigestInvalidLength, digest(format!("sha512:{}", "f".repeat(130)))),
            // An unknown algorithm is unsupported, and a wrong length is reported before
            // uppercase hex. In Go, an algorithm must start with a letter, but the image-spec
            // also allows a digit.
            case(&format!("repo@sha256+b64u:{}", "f".repeat(64)), ParseError::DigestUnsupported, None),
            case(&format!("repo@sha256:{}", "F".repeat(63)), ParseError::DigestInvalidLength, digest(format!("sha256:{}", "F".repeat(63)))),
            case(&format!("repo@2a:{}", "f".repeat(32)), ParseError::ReferenceInvalidFormat, None),
            // Only ASCII letters count as uppercase. Go lowercases with `strings.ToLower`,
            // so it returns `NameContainsUppercase` for the Kelvin sign (U+212A), `İ` and `É`:
            // https://github.com/distribution/reference/blob/v0.6.0/reference.go#L192
            // https://github.com/distribution/reference/blob/v0.6.0/normalize.go#L67
            case("repo:\u{212A}", ParseError::ReferenceInvalidFormat, tag("\u{212A}")),
            case("repo:\u{0130}", ParseError::ReferenceInvalidFormat, tag("\u{0130}")),
            case("É", ParseError::ReferenceInvalidFormat, name("É")),
            case("\u{212A}/app", ParseError::ReferenceInvalidFormat, name("\u{212A}/app")),
            // IPv6 hosts require a repository path, and zone identifiers (`%eth0`) and dotted
            // IPv4 notation such as `::ffff:192.0.2.128` are not allowed. Bracketed hex
            // without a ':' (`[abcdef]`) is not recognized as a domain, so it is parsed as a
            // Docker Hub path and rejected.
            case("[2001:db8::1]", ParseError::ReferenceInvalidFormat, name("[2001:db8::1]")),
            case("[2001:db8::1]:5000", ParseError::ReferenceInvalidFormat, name("[2001:db8::1]")),
            case("[2001:db8::1]:tag", ParseError::ReferenceInvalidFormat, name("[2001:db8::1]")),
            case("[fe80::1%eth0]:5000/repo", ParseError::ReferenceInvalidFormat, None),
            case("[::ffff:192.0.2.128]/repo", ParseError::ReferenceInvalidFormat, None),
            case("[abcdef]/repo", ParseError::ReferenceInvalidFormat, name("[abcdef]/repo")),
            // Tags must be ASCII (`\w` in Go is ASCII only), must not start with `.` or `-`,
            // and are at most 128 characters long: https://pkg.go.dev/regexp/syntax
            case("registry.test/image:-tag", ParseError::ReferenceInvalidFormat, tag("-tag")),
            case("registry.test/image:.tag", ParseError::ReferenceInvalidFormat, tag(".tag")),
            case("registry.test/image:тег", ParseError::ReferenceInvalidFormat, tag("тег")),
            case("registry.test/image:標籤", ParseError::ReferenceInvalidFormat, tag("標籤")),
            case(&format!("registry.test/image:{}", "x".repeat(129)), ParseError::ReferenceInvalidFormat, tag("x".repeat(129))),
        )]
        fn parse_bad_reference(input: &str, err: ParseError, rejected: Option<Part>) {
            assert_rejected(|s| Reference::try_from(s), input, err);
            let (part, spec_accepts) = match &rejected {
                Some(Part::Name(name)) => (name, spec::name(name)),
                Some(Part::Tag(tag)) => (tag, spec::tag(tag)),
                Some(Part::Digest(digest)) => (digest, spec::digest(digest)),
                None => return,
            };
            assert!(
                input.contains(part.as_str()),
                "the test case is wrong: {part:?} is not part of {input:?}"
            );
            assert!(
                !spec_accepts,
                "{rejected:?} is marked as the invalid part of {input:?}, but the spec accepts it"
            );
        }

        /// go-digest v1.0.0 also accepts sha384 digests, which the image-spec does not
        /// register.
        #[test]
        fn parse_sha384_digest() {
            let digest = format!("sha384:{}", "f".repeat(96));
            let input = format!("repo@{digest}");
            let reference = assert_parses(|s| Reference::try_from(s), &input);
            assert_field("repository", &input, "library/repo", reference.repository());
            assert_field("digest", &input, Some(digest.as_str()), reference.digest());
        }

        #[rstest]
        #[case(&format!("a/{}", "a".repeat(253)))]
        #[case(&format!("docker.io/{}/a", "a".repeat(253)))]
        fn parse_name_max_length(#[case] input: &str) {
            let reference = assert_parses(|s| Reference::try_from(s), input);
            assert_field(
                "repository length",
                input,
                255,
                reference.repository().len(),
            );
        }

        #[rstest]
        #[case(&format!("a/{}", "a".repeat(254)))]
        // The length includes the "library/" prefix added for Docker Hub, as in Go:
        // https://github.com/distribution/reference/blob/v0.6.0/reference.go#L209
        #[case(&"a".repeat(248))]
        fn parse_name_too_long(#[case] input: &str) {
            assert_rejected(|s| Reference::try_from(s), input, ParseError::NameTooLong);
        }

        #[test]
        fn parse_tag_max_length() {
            let tag = "x".repeat(128);
            let input = format!("registry.test/image:{tag}");
            let reference = assert_parses(|s| Reference::try_from(s), &input);
            assert_field("registry", &input, "registry.test", reference.registry());
            assert_field("repository", &input, "image", reference.repository());
            assert_field("tag", &input, Some(tag.as_str()), reference.tag());
            assert_field("digest", &input, None, reference.digest());
            assert_field("whole()", &input, &input, &reference.whole());
        }

        #[rstest(
            input,
            registry,
            resolved_registry,
            whole,
            case(
                "busybox",
                "docker.io",
                "index.docker.io",
                "docker.io/library/busybox:latest"
            ),
            case("test.com/repo:tag", "test.com", "test.com", "test.com/repo:tag"),
            case("test:5000/repo", "test:5000", "test:5000", "test:5000/repo:latest"),
            case(
                "sub-dom1.foo.com/bar/baz/quux",
                "sub-dom1.foo.com",
                "sub-dom1.foo.com",
                "sub-dom1.foo.com/bar/baz/quux:latest"
            ),
            case(
                "b.gcr.io/test.example.com/my-app:test.example.com",
                "b.gcr.io",
                "b.gcr.io",
                "b.gcr.io/test.example.com/my-app:test.example.com"
            )
        )]
        fn test_mirror_registry(input: &str, registry: &str, resolved_registry: &str, whole: &str) {
            let mut reference = Reference::try_from(input).expect("could not parse reference");
            assert_eq!(resolved_registry, reference.resolve_registry());
            assert_eq!(registry, reference.registry());
            assert_eq!(None, reference.namespace());
            assert_eq!(whole, reference.whole());

            reference.set_mirror_registry("docker.mirror.io".to_owned());
            assert_eq!("docker.mirror.io", reference.resolve_registry());
            assert_eq!(registry, reference.registry());
            assert_eq!(Some(registry), reference.namespace());
            assert_eq!(whole, reference.whole());
        }

        #[rstest(
            expected, registry, repository, tag, digest,
            case(
                "docker.io/foo/bar:1.2@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 
                "docker.io", 
                "foo/bar", 
                "1.2", 
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            )
        )]
        fn test_create_reference_from_tag_and_digest(
            expected: &str,
            registry: &str,
            repository: &str,
            tag: &str,
            digest: &str,
        ) {
            let reference = Reference::with_tag_and_digest(
                registry.to_string(),
                repository.to_string(),
                tag.to_string(),
                digest.to_string(),
            );
            assert_eq!(expected, reference.to_string());
        }
    }

    /// Checks the hand-written parser against the regular expressions in the OCI specs.
    ///
    /// Inputs are all short strings made from a few chosen characters, so these tests are
    /// deterministic.
    mod spec_regex {
        use super::*;
        use regex::Regex;

        use super::spec::{DIGEST as SPEC_DIGEST, NAME as SPEC_NAME, TAG as SPEC_TAG};

        #[test]
        fn name_matches_spec() {
            let spec = Regex::new(SPEC_NAME).unwrap();
            let alphabet = ['a', '0', 'A', '.', '_', '-', '/', 'é'];
            for s in all_strings(&alphabet, 7) {
                let spec_accepts = spec.is_match(&s);
                assert!(
                    spec_accepts == is_remote_name(&s),
                    "{}",
                    disagree(
                        &s,
                        "the spec regex for <name>",
                        spec_accepts,
                        "is_remote_name"
                    )
                );
            }
        }

        #[test]
        fn tag_matches_spec() {
            let spec = Regex::new(SPEC_TAG).unwrap();
            let alphabet = ['a', 'Z', '0', '_', '.', '-', ':', '/', 'é'];
            let mut inputs = all_strings(&alphabet, 6);
            for len in [127, 128, 129] {
                inputs.push("a".repeat(len));
                inputs.push(format!("{}é", "a".repeat(len - 1)));
            }
            for s in inputs {
                let spec_accepts = spec.is_match(&s);
                assert!(
                    spec_accepts == is_tag(&s),
                    "{}",
                    disagree(&s, "the spec regex for tags", spec_accepts, "is_tag")
                );
            }
        }

        #[test]
        fn digest_matches_spec() {
            let spec = Regex::new(SPEC_DIGEST).unwrap();
            let hex = |len| Regex::new(&format!("^[a-f0-9]{{{len}}}$")).unwrap();
            let (sha256, sha384, sha512) = (hex(64), hex(96), hex(128));

            let mut algorithms = all_strings(&['s', 'A', '2', '+', '.', '_', '-', ':'], 4);
            algorithms.extend(
                [
                    "sha256",
                    "sha384",
                    "sha512",
                    "Sha256",
                    "SHA512",
                    "sha256+b64u",
                    "blake3",
                ]
                .map(String::from),
            );
            let mut encoded = Vec::new();
            for len in [31, 32, 63, 64, 65, 96, 128, 129] {
                encoded.push("f".repeat(len));
                for c in ['0', '9', 'a', 'g', 'A', 'F', '=', '_', '-', ':', '@', 'é'] {
                    for i in [0, len / 2, len - 1] {
                        let mut e: Vec<char> = "f".repeat(len).chars().collect();
                        e[i] = c;
                        encoded.push(e.into_iter().collect());
                    }
                }
            }

            for algorithm in &algorithms {
                for encoded in &encoded {
                    let d = format!("{algorithm}:{encoded}");
                    let ours = is_digest(&d) && validate_digest(&d).is_ok();
                    let in_spec = spec.is_match(&d)
                        && match algorithm.as_str() {
                            "sha256" => sha256.is_match(encoded),
                            "sha512" => sha512.is_match(encoded),
                            // Not in the image-spec, but accepted by go-digest v1.0.0:
                            // https://github.com/opencontainers/go-digest/blob/v1.0.0/algorithm.go#L52
                            "sha384" => sha384.is_match(encoded),
                            // Other algorithms, such as `blake3`, may be valid in a
                            // descriptor but are not supported in a reference:
                            // https://github.com/opencontainers/image-spec/blob/v1.1.1/descriptor.md#registered-algorithms
                            _ => false,
                        };
                    assert!(
                        in_spec == ours,
                        "{}",
                        disagree(
                            &d,
                            "the image-spec digest grammar (plus sha384)",
                            in_spec,
                            "is_digest with validate_digest"
                        )
                    );

                    #[cfg(feature = "image")]
                    if ours {
                        if let Err(e) = d.parse::<crate::image::Digest>() {
                            panic!(
                                "{d:?} is valid in a reference, but image::Digest rejects it: {e}"
                            );
                        }
                    }
                }
            }
        }

        /// Every accepted reference has a repository, tag and digest that the specs
        /// accept, and parses again to the same reference.
        #[test]
        fn parsed_parts_match_spec() {
            let (name, tag, digest) = (
                Regex::new(SPEC_NAME).unwrap(),
                Regex::new(SPEC_TAG).unwrap(),
                Regex::new(SPEC_DIGEST).unwrap(),
            );
            let alphabet = ['a', '0', 'A', '.', '_', '-', '/', ':', '@'];
            let suffixes = ["", ":v1", &format!("@sha256:{}", "f".repeat(64))];
            let mut accepted = 0;
            for s in all_strings(&alphabet, 6) {
                for suffix in suffixes {
                    let input = format!("{s}{suffix}");
                    let Ok(r) = Reference::try_from(input.as_str()) else {
                        continue;
                    };
                    accepted += 1;
                    assert!(
                        name.is_match(r.repository()),
                        "{input:?} parses as {r:?}, but the spec regex rejects its repository"
                    );
                    assert!(
                        r.tag().is_none_or(|t| tag.is_match(t)),
                        "{input:?} parses as {r:?}, but the spec regex rejects its tag"
                    );
                    assert!(
                        r.digest().is_none_or(|d| digest.is_match(d)),
                        "{input:?} parses as {r:?}, but the spec regex rejects its digest"
                    );
                    let whole = r.whole();
                    let reparsed = Reference::try_from(whole.as_str());
                    assert!(
                        reparsed.as_ref() == Ok(&r),
                        "{input:?} parses as {r:?}, but its whole() {whole:?} parses as {reparsed:?}"
                    );
                }
            }
            // Make sure the alphabet produces enough valid references to mean something.
            assert!(
                accepted > 10_000,
                "only {accepted} inputs are valid references, too few for this test to mean much"
            );
        }
    }

    /// Checks the hand-written parser against the regular expressions of
    /// distribution/reference (Go), see the `go` module. This covers the registry host,
    /// which the specs have no rule for. Inputs are all short strings made from a few
    /// chosen characters.
    mod go_regex {
        use super::*;

        #[test]
        fn domain_matches_go() {
            let go = go::domain_and_port_regexp();
            for s in all_strings(&['a', 'Z', '0', '-', '.', ':', '[', ']'], 6) {
                let go_accepts = go.is_match(&s);
                assert!(
                    go_accepts == is_domain_and_port(&s),
                    "{}",
                    disagree(&s, "Go's domainAndPort", go_accepts, "is_domain_and_port")
                );
            }
        }

        #[test]
        fn digest_matches_go() {
            let go = go::digest_regexp();
            let hex = [
                "f".repeat(31),
                "f".repeat(32),
                "F".repeat(32),
                format!("{}g", "f".repeat(31)),
            ];
            for algorithm in all_strings(&['a', 'Z', '2', '+', '.', '_', '-', ':', 'é'], 5) {
                for hex in &hex {
                    let s = format!("{algorithm}:{hex}");
                    let go_accepts = go.is_match(&s);
                    assert!(
                        go_accepts == is_digest(&s),
                        "{}",
                        disagree(&s, "Go's digestPat", go_accepts, "is_digest")
                    );
                }
            }
        }

        #[test]
        fn name_split_matches_go() {
            let go = go::anchored_name_regexp();
            for s in all_strings(&['a', 'A', '0', '.', '-', '_', ':', '/', '[', ']'], 6) {
                let go_split = go.captures(&s).map(|c| {
                    (
                        c.get(1).map_or("", |m| m.as_str()),
                        c.get(2).unwrap().as_str(),
                    )
                });
                assert!(
                    go_split.is_some() == is_name(&s),
                    "{}",
                    disagree(&s, "Go's anchoredNameRegexp", go_split.is_some(), "is_name")
                );
                if let Some(go_split) = go_split {
                    let ours = split_name(&s);
                    assert!(
                        go_split == ours,
                        "Go splits {s:?} into (registry, repository) {go_split:?}, but split_name gives {ours:?}"
                    );
                }
            }
        }

        #[test]
        fn reference_matches_go() {
            let go = go::reference_regexp();
            let f = "f".repeat(64);
            let suffixes = [
                String::new(),
                ":v1".to_owned(),
                format!("@sha256:{f}"),
                format!(":v1@sha256:{f}"),
                format!("@Sha256:{f}"),
                format!("@sha256:{}", "f".repeat(31)),
            ];
            let mut accepted = 0;
            for s in all_strings(&['a', 'A', '0', '.', '-', ':', '/', '@', '[', ']', 'é'], 5) {
                for suffix in &suffixes {
                    let input = format!("{s}{suffix}");
                    let expected = go.captures(&input).map(|c| {
                        (
                            c.get(1).unwrap().as_str(),
                            c.get(2).map(|m| m.as_str()),
                            c.get(3).map(|m| m.as_str()),
                        )
                    });
                    accepted += usize::from(expected.is_some());
                    let ours = match_reference(&input);
                    assert!(
                        expected.is_some() == ours.is_some(),
                        "{}",
                        disagree(
                            &input,
                            "Go's referenceRegexp",
                            expected.is_some(),
                            "match_reference"
                        )
                    );
                    assert!(
                        expected == ours,
                        "Go splits {input:?} into (name, tag, digest) {expected:?}, but match_reference gives {ours:?}"
                    );
                }
            }
            // Make sure the inputs produce enough valid references to mean something.
            assert!(
                accepted > 3_000,
                "only {accepted} inputs are valid references, too few for this test to mean much"
            );
        }
    }
}
