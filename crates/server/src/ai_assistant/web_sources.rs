use std::collections::HashSet;

use futures::future::join_all;
use serde::Serialize;

use super::web::{
    PublicWebPageSummary, PublicWebSearchResult, fetch_public_page_summary, normalize_public_url,
    search_public_web,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CuratedWebCategory {
    Technology,
    Business,
    Economics,
}

impl CuratedWebCategory {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Technology => "technology",
            Self::Business => "business",
            Self::Economics => "economics",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Technology => "Technology",
            Self::Business => "Business",
            Self::Economics => "Economics",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Technology => "Technology news, engineering, product, and developer sources.",
            Self::Business => "Business and company news sources.",
            Self::Economics => {
                "Macroeconomic, labor, inflation, policy, and official data sources."
            }
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug.trim().to_ascii_lowercase().as_str() {
            "technology" | "tech" | "engineering" | "developer" => Some(Self::Technology),
            "business" | "biz" | "companies" | "company" => Some(Self::Business),
            "economics" | "economy" | "economic" | "macro" | "macroeconomics" => {
                Some(Self::Economics)
            }
            _ => None,
        }
    }

    pub fn from_message(message_lower: &str) -> Option<Self> {
        if has_any(
            message_lower,
            &[
                "economy",
                "economic",
                "economics",
                "inflation",
                "cpi",
                "pce",
                "gdp",
                "unemployment",
                "jobs report",
                "labor market",
                "interest rate",
                "interest rates",
                "federal reserve",
                "fed",
                "central bank",
                "monetary policy",
                "recession",
                "macro",
            ],
        ) {
            return Some(Self::Economics);
        }

        if has_any(
            message_lower,
            &[
                "business",
                "company",
                "companies",
                "startup",
                "funding",
                "earnings",
                "revenue",
                "sales",
                "market",
                "markets",
                "stock",
                "stocks",
                "ipo",
                "acquisition",
                "merger",
                "guidance",
                "ceo",
                "product launch",
            ],
        ) {
            return Some(Self::Business);
        }

        if has_any(
            message_lower,
            &[
                "technology",
                "tech",
                "software",
                "programming",
                "developer",
                "developers",
                "api",
                "sdk",
                "open source",
                "artificial intelligence",
                "machine learning",
                "ai ",
                "chip",
                "chips",
                "semiconductor",
                "cloud",
                "documentation",
                "docs",
                "browser",
                "release notes",
                "engineering",
                "linux",
                "rust",
            ],
        ) {
            return Some(Self::Technology);
        }

        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CuratedWebSource {
    pub name: &'static str,
    pub domains: &'static [&'static str],
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct CuratedWebCategorySpec {
    pub category: CuratedWebCategory,
    pub label: &'static str,
    pub description: &'static str,
    pub sources: &'static [CuratedWebSource],
}

#[derive(Debug, Clone, Serialize)]
pub struct CuratedWebSourceSummary {
    pub name: &'static str,
    pub domains: &'static [&'static str],
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CuratedWebCategorySummary {
    pub category: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub source_count: usize,
    pub sources: Vec<CuratedWebSourceSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CuratedWebCatalogSummary {
    pub categories: Vec<CuratedWebCategorySummary>,
}

const TECHNOLOGY_SOURCES: &[CuratedWebSource] = &[
    CuratedWebSource {
        name: "Ars Technica",
        domains: &["arstechnica.com"],
        description: "Long-form technology reporting and analysis.",
    },
    CuratedWebSource {
        name: "TechCrunch",
        domains: &["techcrunch.com"],
        description: "Technology company, startup, and product coverage.",
    },
    CuratedWebSource {
        name: "The Verge",
        domains: &["theverge.com"],
        description: "Consumer technology and platform coverage.",
    },
    CuratedWebSource {
        name: "MIT Technology Review",
        domains: &["technologyreview.com"],
        description: "Technology analysis and emerging technology reporting.",
    },
    CuratedWebSource {
        name: "GitHub Blog",
        domains: &["github.blog"],
        description: "Software engineering and platform announcements.",
    },
    CuratedWebSource {
        name: "Cloudflare Blog",
        domains: &["blog.cloudflare.com"],
        description: "Networking, security, and internet infrastructure updates.",
    },
];

const BUSINESS_SOURCES: &[CuratedWebSource] = &[
    CuratedWebSource {
        name: "Reuters Business",
        domains: &["reuters.com"],
        description: "Breaking business and company news.",
    },
    CuratedWebSource {
        name: "CNBC",
        domains: &["cnbc.com"],
        description: "Markets, companies, and business news.",
    },
    CuratedWebSource {
        name: "AP News Business",
        domains: &["apnews.com"],
        description: "General business news and reporting.",
    },
    CuratedWebSource {
        name: "Fortune",
        domains: &["fortune.com"],
        description: "Business and leadership reporting.",
    },
];

const ECONOMICS_SOURCES: &[CuratedWebSource] = &[
    CuratedWebSource {
        name: "FRED",
        domains: &["fred.stlouisfed.org"],
        description: "Federal Reserve Economic Data series and charts.",
    },
    CuratedWebSource {
        name: "BLS",
        domains: &["bls.gov"],
        description: "US labor, inflation, and price statistics.",
    },
    CuratedWebSource {
        name: "BEA",
        domains: &["bea.gov"],
        description: "US GDP, income, and output statistics.",
    },
    CuratedWebSource {
        name: "Federal Reserve",
        domains: &["federalreserve.gov"],
        description: "US central bank policy, statements, and data.",
    },
    CuratedWebSource {
        name: "World Bank",
        domains: &["worldbank.org"],
        description: "Global development and economic indicators.",
    },
];

const TECHNOLOGY_SPEC: CuratedWebCategorySpec = CuratedWebCategorySpec {
    category: CuratedWebCategory::Technology,
    label: "Technology",
    description: "Technology news, engineering, product, and developer sources.",
    sources: TECHNOLOGY_SOURCES,
};

const BUSINESS_SPEC: CuratedWebCategorySpec = CuratedWebCategorySpec {
    category: CuratedWebCategory::Business,
    label: "Business",
    description: "Business and company news sources.",
    sources: BUSINESS_SOURCES,
};

const ECONOMICS_SPEC: CuratedWebCategorySpec = CuratedWebCategorySpec {
    category: CuratedWebCategory::Economics,
    label: "Economics",
    description: "Macroeconomic, labor, inflation, policy, and official data sources.",
    sources: ECONOMICS_SOURCES,
};

pub fn curated_web_categories() -> &'static [CuratedWebCategorySpec] {
    &[TECHNOLOGY_SPEC, BUSINESS_SPEC, ECONOMICS_SPEC]
}

pub fn curated_web_catalog_summary() -> CuratedWebCatalogSummary {
    CuratedWebCatalogSummary {
        categories: curated_web_categories()
            .iter()
            .map(|spec| CuratedWebCategorySummary {
                category: spec.category.slug(),
                label: spec.label,
                description: spec.description,
                source_count: spec.sources.len(),
                sources: spec
                    .sources
                    .iter()
                    .map(|source| CuratedWebSourceSummary {
                        name: source.name,
                        domains: source.domains,
                        description: source.description,
                    })
                    .collect(),
            })
            .collect(),
    }
}

pub fn curated_web_category_spec(category: CuratedWebCategory) -> &'static CuratedWebCategorySpec {
    match category {
        CuratedWebCategory::Technology => &TECHNOLOGY_SPEC,
        CuratedWebCategory::Business => &BUSINESS_SPEC,
        CuratedWebCategory::Economics => &ECONOMICS_SPEC,
    }
}

pub fn curated_web_category_label(category: &str) -> Option<&'static str> {
    CuratedWebCategory::from_slug(category).map(|category| category.label())
}

pub fn curated_web_category_for_message(message_lower: &str) -> Option<&'static str> {
    CuratedWebCategory::from_message(message_lower).map(|category| category.slug())
}

pub fn curated_web_category_for_url(raw_url: &str) -> Option<&'static str> {
    let url = normalize_public_url(raw_url).ok()?;
    let host = url.host_str()?.trim().to_ascii_lowercase();
    curated_web_category_for_host(&host).map(|category| category.slug())
}

pub fn curated_web_category_for_host(host: &str) -> Option<CuratedWebCategory> {
    let host = host.trim().to_ascii_lowercase();
    curated_web_categories().iter().find_map(|spec| {
        if spec.sources.iter().any(|source| {
            source
                .domains
                .iter()
                .any(|domain| host_matches_domain(&host, domain))
        }) {
            Some(spec.category)
        } else {
            None
        }
    })
}

pub async fn search_curated_web(
    category: CuratedWebCategory,
    query: &str,
) -> Result<Vec<PublicWebSearchResult>, String> {
    let normalized_query = normalize_curated_search_query(query)?;
    let spec = curated_web_category_spec(category);
    let mut futures = Vec::new();
    for source in spec.sources {
        for domain in source.domains {
            let site_query = format!("site:{domain} {normalized_query}");
            futures.push(async move { search_public_web(&site_query, Some(2)).await });
        }
    }

    let mut results = Vec::new();
    let mut seen_urls = HashSet::new();
    let mut errors = Vec::new();
    for batch in join_all(futures).await {
        match batch {
            Ok(items) => {
                for item in items {
                    if !host_matches_any(item.source_host.as_str(), spec.sources) {
                        continue;
                    }
                    if seen_urls.insert(item.url.clone()) {
                        results.push(item);
                    }
                }
            }
            Err(error) => errors.push(error),
        }
    }

    if results.is_empty() {
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        return Err(format!(
            "{} curated web sources returned no results for \"{}\"",
            spec.label, normalized_query
        ));
    }

    results.truncate(8);
    Ok(results)
}

pub async fn fetch_curated_web_page_summary(
    category: CuratedWebCategory,
    raw_url: &str,
) -> Result<PublicWebPageSummary, String> {
    let normalized_url = normalize_public_url(raw_url)?;
    let host = normalized_url
        .host_str()
        .ok_or_else(|| "public web URL is missing a host".to_string())?
        .trim()
        .to_ascii_lowercase();
    if !curated_web_category_for_host(&host)
        .map(|matched| matched == category)
        .unwrap_or(false)
    {
        return Err(format!(
            "{} is not an allowed {} source",
            host,
            category.label().to_ascii_lowercase()
        ));
    }

    fetch_public_page_summary(normalized_url.as_str()).await
}

fn normalize_curated_search_query(raw_query: &str) -> Result<String, String> {
    let normalized = raw_query.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = normalized.trim().to_string();
    if normalized.chars().count() < 3 {
        return Err("curated web search query must be at least 3 characters".to_string());
    }
    Ok(truncate_chars(&normalized, 96))
}

fn host_matches_any(host: &str, sources: &[CuratedWebSource]) -> bool {
    sources.iter().any(|source| {
        source
            .domains
            .iter()
            .any(|domain| host_matches_domain(host, domain))
    })
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let domain = domain.trim().to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn has_any(message_lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| message_lower.contains(needle))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        CuratedWebCategory, curated_web_catalog_summary, curated_web_category_for_host,
        curated_web_category_for_message, curated_web_category_for_url,
    };

    #[test]
    fn curated_catalog_exposes_expected_categories() {
        let catalog = curated_web_catalog_summary();
        assert_eq!(catalog.categories.len(), 3);

        let labels = catalog
            .categories
            .iter()
            .map(|category| category.label)
            .collect::<Vec<_>>();
        assert!(labels.contains(&"Technology"));
        assert!(labels.contains(&"Business"));
        assert!(labels.contains(&"Economics"));
    }

    #[test]
    fn curated_category_inference_prefers_specific_domains() {
        assert_eq!(
            curated_web_category_for_message("What technology sites should I check for Rust news?"),
            Some("technology")
        );
        assert_eq!(
            curated_web_category_for_message(
                "What business sites should I check for company earnings?"
            ),
            Some("business")
        );
        assert_eq!(
            curated_web_category_for_message(
                "What economic data should I use for inflation and CPI?"
            ),
            Some("economics")
        );
    }

    #[test]
    fn curated_category_inference_matches_hosts() {
        assert_eq!(
            curated_web_category_for_url("https://arstechnica.com/gadgets/"),
            Some("technology")
        );
        assert_eq!(
            curated_web_category_for_url("https://www.reuters.com/markets/"),
            Some("business")
        );
        assert_eq!(
            curated_web_category_for_url("https://www.federalreserve.gov/monetarypolicy.htm"),
            Some("economics")
        );
        assert_eq!(
            curated_web_category_for_host("blog.cloudflare.com"),
            Some(CuratedWebCategory::Technology)
        );
    }
}
