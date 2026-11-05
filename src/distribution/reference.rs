use std::fmt;
use std::str::FromStr;
use std::{convert::TryFrom, sync::OnceLock};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// NAME_TOTAL_LENGTH_MAX is the maximum total number of characters in a repository name.
const NAME_TOTAL_LENGTH_MAX: usize = 255;

const DOCKER_HUB_DOMAIN_LEGACY: &str = "index.docker.io";
const DOCKER_HUB_DOMAIN: &str = "docker.io";
const DOCKER_HUB_OFFICIAL_REPO_NAME: &str = "library";
const DEFAULT_TAG: &str = "latest";
/// REFERENCE_REGEXP is the full supported format of a reference. The regexp
/// is anchored and has capturing groups for name, tag, and digest components.
const REFERENCE_REGEXP: &str = r"^((?:(?:[a-zA-Z0-9]|[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9])(?:(?:\.(?:[a-zA-Z0-9]|[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9]))+)?(?::[0-9]+)?/)?[a-z0-9]+(?:(?:(?:[._]|__|[-]*)[a-z0-9]+)+)?(?:(?:/[a-z0-9]+(?:(?:(?:[._]|__|[-]*)[a-z0-9]+)+)?)+)?)(?::([\w][\w.-]{0,127}))?(?:@([A-Za-z][A-Za-z0-9]*(?:[-_+.][A-Za-z][A-Za-z0-9]*)*[:][[:xdigit:]]{32,}))?$";

fn reference_regexp() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(REFERENCE_REGEXP)
            .size_limit(10 * (1 << 21))
            .build()
            .unwrap()
    })
}

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
    pub fn resolve_registry(&self) -> &str {
        match (self.registry(), self.mirror_registry.as_deref()) {
            (_, Some(mirror_registry)) => mirror_registry,
            ("docker.io", None) => "index.docker.io",
            (registry, None) => registry,
        }
    }

    /// Returns the name of the registry.
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

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(ParseError::NameEmpty);
        }
        let captures = match reference_regexp().captures(s) {
            Some(caps) => caps,
            None => {
                return Err(ParseError::ReferenceInvalidFormat);
            }
        };
        let name = &captures[1];
        let mut tag = captures.get(2).map(|m| m.as_str().to_owned());
        let digest = captures.get(3).map(|m| m.as_str().to_owned());
        if tag.is_none() && digest.is_none() {
            tag = Some(DEFAULT_TAG.into());
        }
        let (registry, repository) = split_domain(name);
        let reference = Reference {
            registry,
            mirror_registry: None,
            repository,
            tag,
            digest,
        };
        if reference.repository().len() > NAME_TOTAL_LENGTH_MAX {
            return Err(ParseError::NameTooLong);
        }
        // Digests much always be hex-encoded, ensuring that their hex portion will always be
        // size*2
        if let Some(digest) = reference.digest() {
            match digest.split_once(':') {
                None => return Err(ParseError::DigestInvalidFormat),
                Some(("sha256", digest)) => {
                    if digest.len() != 64 {
                        return Err(ParseError::DigestInvalidLength);
                    }
                }
                Some(("sha384", digest)) => {
                    if digest.len() != 96 {
                        return Err(ParseError::DigestInvalidLength);
                    }
                }
                Some(("sha512", digest)) => {
                    if digest.len() != 128 {
                        return Err(ParseError::DigestInvalidLength);
                    }
                }
                Some((_, _)) => return Err(ParseError::DigestUnsupported),
            }
        }
        Ok(reference)
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

/// Splits a repository name to domain and remotename string.
/// If no valid domain is found, the default domain is used. Repository name
/// needs to be already validated before.
///
/// This function is a Rust rewrite of the official Go code used by Docker:
/// https://github.com/distribution/distribution/blob/41a0452eea12416aaf01bceb02a924871e964c67/reference/normalize.go#L87-L104
fn split_domain(name: &str) -> (String, String) {
    let mut domain: String;
    let mut remainder: String;

    match name.split_once('/') {
        None => {
            domain = DOCKER_HUB_DOMAIN.into();
            remainder = name.into();
        }
        Some((left, right)) => {
            if !(left.contains('.') || left.contains(':')) && left != "localhost" {
                domain = DOCKER_HUB_DOMAIN.into();
                remainder = name.into();
            } else {
                domain = left.into();
                remainder = right.into();
            }
        }
    }
    if domain == DOCKER_HUB_DOMAIN_LEGACY {
        domain = DOCKER_HUB_DOMAIN.into();
    }
    if domain == DOCKER_HUB_DOMAIN && !remainder.contains('/') {
        remainder = format!("{DOCKER_HUB_OFFICIAL_REPO_NAME}/{remainder}");
    }

    (domain, remainder)
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

    mod parse {
        use super::*;
        use rstest::rstest;

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
            case("localhost/app", "localhost", "app", Some("latest"), None, "localhost/app:latest"),
            case("index.docker.io/busybox", "docker.io", "library/busybox", Some("latest"), None, "docker.io/library/busybox:latest"),
            case("a--b/c__d.e_f", "docker.io", "a--b/c__d.e_f", Some("latest"), None, "docker.io/a--b/c__d.e_f:latest"),
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
            // FIXME: should really pass a ParseError::NameContainsUppercase, but "invalid format" is good enough for now.
            case("Uppercase:tag", ParseError::ReferenceInvalidFormat, name("Uppercase")),
            // FIXME: "Uppercase" is incorrectly handled as a domain-name here, and therefore passes.
            // https://github.com/docker/distribution/blob/master/reference/reference_test.go#L104-L109
            // case("Uppercase/lowercase:tag", ParseError::NameContainsUppercase, name("Uppercase/lowercase")),
            // FIXME: should really pass a ParseError::NameContainsUppercase, but "invalid format" is good enough for now.
            case("test:5000/Uppercase/lowercase:tag", ParseError::ReferenceInvalidFormat, name("Uppercase/lowercase")),
            case(&"a".repeat(256), ParseError::NameTooLong, None),
            case("aa/asdf$$^/aa", ParseError::ReferenceInvalidFormat, name("aa/asdf$$^/aa")),
            // Invalid hosts.
            case("[::1/app", ParseError::ReferenceInvalidFormat, None),
            case("[::g]/app", ParseError::ReferenceInvalidFormat, None),
            case("-a.com/app", ParseError::ReferenceInvalidFormat, None),
            // Unicode domains are not supported.
            case("bücher.example/app", ParseError::ReferenceInvalidFormat, None),
            case("☃.com/myimage", ParseError::ReferenceInvalidFormat, None),
            // Only sha256, sha384 and sha512 are supported.
            case("repo@md5:ffffffffffffffffffffffffffffffff", ParseError::DigestUnsupported, None),
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
            // IPv6 hosts require a repository path. Zone identifiers (`%eth0`), dotted IPv4
            // notation such as `::ffff:192.0.2.128`, and bracketed hex without a ':' are not
            // registry hosts.
            case("[2001:db8::1]", ParseError::ReferenceInvalidFormat, name("[2001:db8::1]")),
            case("[2001:db8::1]:5000", ParseError::ReferenceInvalidFormat, name("[2001:db8::1]")),
            case("[2001:db8::1]:tag", ParseError::ReferenceInvalidFormat, name("[2001:db8::1]")),
            case("[fe80::1%eth0]:5000/repo", ParseError::ReferenceInvalidFormat, None),
            case("[::ffff:192.0.2.128]/repo", ParseError::ReferenceInvalidFormat, None),
            case("[abcdef]/repo", ParseError::ReferenceInvalidFormat, name("[abcdef]/repo")),
            // Tags must not start with `.` or `-`, and are at most 128 characters long.
            case("registry.test/image:-tag", ParseError::ReferenceInvalidFormat, tag("-tag")),
            case("registry.test/image:.tag", ParseError::ReferenceInvalidFormat, tag(".tag")),
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
}
