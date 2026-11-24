use clap::{Args, Parser, Subcommand, ValueEnum};

macro_rules! values {
    ($name:ident { $($value:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
        pub enum $name { $($value),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$value => $wire),+ }
            }
        }
    };
}

values!(Format { Auto => "auto", Json => "json", Jsonl => "jsonl", Md => "md", Pretty => "pretty" });
values!(Source { Auto => "auto", Live => "live", Archive => "archive" });
values!(SubSort { Hot => "hot", New => "new", Top => "top", Rising => "rising", Controversial => "controversial" });
values!(Period { Hour => "hour", Day => "day", Week => "week", Month => "month", Year => "year", All => "all" });
values!(SearchType { Posts => "posts", Comments => "comments", Subs => "subs", Users => "users" });
values!(SearchSort { Relevance => "relevance", Hot => "hot", Top => "top", New => "new", Comments => "comments" });
values!(Rank { Interleave => "interleave", Score => "score", New => "new", Comments => "comments" });
values!(ThreadSort { Confidence => "confidence", Top => "top", New => "new", Controversial => "controversial", Old => "old", Qa => "qa" });
values!(UserSort { New => "new", Hot => "hot", Top => "top", Controversial => "controversial" });
values!(CacheKind { Post => "post", Comment => "comment" });

#[derive(Debug, Parser)]
#[command(
    name = "reddit",
    version,
    about = "Read-only Reddit research over the Reddit API (live) or the Arctic Shift archive"
)]
pub struct Cli {
    /// Choose how to render results.
    #[arg(long, global = true, value_enum)]
    pub format: Option<Format>,
    /// Choose the live API or the archive.
    #[arg(long, global = true, value_enum)]
    pub source: Option<Source>,
    /// Use only the local store and ignore cache expiration.
    #[arg(long, global = true)]
    pub offline: bool,
    /// Fetch fresh results and update the cache.
    #[arg(long, global = true)]
    pub refresh: bool,
    /// Skip cache reads and writes.
    #[arg(long, global = true)]
    pub no_cache: bool,
    /// Suppress notes on stderr.
    #[arg(short, long, global = true)]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage live Reddit credentials.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// List posts from a subreddit.
    Sub(SubArgs),
    /// Show a subreddit profile.
    About {
        /// Subreddit name.
        name: String,
    },
    /// Search posts, comments, subreddits, or users.
    Search(SearchArgs),
    /// Read a post and its comment tree.
    Thread(ThreadArgs),
    /// Show a user profile and activity.
    User(UserArgs),
    /// Inspect or manage the local store.
    #[command(subcommand)]
    Cache(CacheCommand),
    /// Read the built-in agent skills.
    #[command(subcommand)]
    Skills(SkillsCommand),
    /// Generate a shell completion script.
    Completions {
        /// Shell to generate the script for.
        shell: clap_complete::Shell,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Show credential, token, and rate-limit status.
    Status,
    /// Save credentials and request a live API token.
    Login {
        /// OAuth application client ID.
        #[arg(long)]
        client_id: String,
        /// Read the client secret from standard input.
        #[arg(long)]
        client_secret_stdin: bool,
        /// Set the HTTP User-Agent for live requests.
        #[arg(long)]
        user_agent: Option<String>,
    },
    /// Remove saved credentials and the cached token.
    Logout,
}

#[derive(Debug, Args)]
pub struct SubArgs {
    /// Subreddit name.
    pub name: String,
    /// Post order. Defaults to hot for live and new for archive.
    #[arg(long, value_enum)]
    pub sort: Option<SubSort>,
    /// Time period for the listing.
    #[arg(long, value_enum)]
    pub time: Option<Period>,
    /// Maximum number of posts to return.
    #[arg(long, default_value_t = 25, value_parser = limit)]
    pub limit: usize,
    /// Resume from a previous page cursor.
    #[arg(long)]
    pub after: Option<String>,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Words or name prefix to search for.
    pub query: String,
    /// Restrict the search to a subreddit. Repeat to search multiple subreddits.
    #[arg(short, long)]
    pub sub: Vec<String>,
    /// Choose what to search for.
    #[arg(long = "type", value_enum, default_value = "posts")]
    pub search_type: SearchType,
    /// Order results within each search.
    #[arg(long, value_enum, default_value = "relevance")]
    pub sort: SearchSort,
    /// Set the time period for live search.
    #[arg(long, value_enum)]
    pub time: Option<Period>,
    /// Restrict archive results to a username.
    #[arg(long)]
    pub author: Option<String>,
    /// Restrict archive results to items after this date.
    #[arg(long, allow_negative_numbers = true)]
    pub since: Option<String>,
    /// Restrict archive results to items before this date.
    #[arg(long, allow_negative_numbers = true)]
    pub until: Option<String>,
    /// Maximum number of results to return.
    #[arg(long, default_value_t = 25, value_parser = limit)]
    pub limit: usize,
    /// Resume from a previous page cursor.
    #[arg(long)]
    pub after: Option<String>,
    /// Order results after merging subreddit searches.
    #[arg(long, value_enum, default_value = "interleave")]
    pub rank: Rank,
    /// Attach up to this many top root comments per post.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=100))]
    pub comments: u8,
}

#[derive(Debug, Args)]
pub struct ThreadArgs {
    /// Reddit post URL or ID.
    pub url_or_id: String,
    /// Focus the thread on this comment ID.
    #[arg(long)]
    pub comment: Option<String>,
    /// Order comments within the thread.
    #[arg(long, value_enum)]
    pub sort: Option<ThreadSort>,
    /// Maximum comment depth, with roots at zero.
    #[arg(long)]
    pub depth: Option<usize>,
    /// Maximum number of comments to return.
    #[arg(long, default_value_t = 200, value_parser = limit)]
    pub limit: usize,
    /// Expand collapsed live replies.
    #[arg(long)]
    pub full: bool,
    /// Maximum requests for live thread expansion.
    #[arg(long)]
    pub max_requests: Option<usize>,
    /// Write the rendered thread to this file.
    #[arg(short)]
    pub output: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
#[group(skip)]
pub struct UserArgs {
    /// Reddit username.
    pub name: String,
    /// List posts only.
    #[arg(long, group = "listing")]
    pub posts: bool,
    /// List comments only.
    #[arg(long, group = "listing")]
    pub comments: bool,
    /// List both posts and comments.
    #[arg(long, group = "listing")]
    pub overview: bool,
    /// Order the user activity listing.
    #[arg(long, value_enum, default_value = "new")]
    pub sort: UserSort,
    /// Set the time period for the live listing.
    #[arg(long, value_enum)]
    pub time: Option<Period>,
    /// Restrict archive results to items after this date.
    #[arg(long, allow_negative_numbers = true)]
    pub since: Option<String>,
    /// Restrict archive results to items before this date.
    #[arg(long, allow_negative_numbers = true)]
    pub until: Option<String>,
    /// Maximum number of items to return.
    #[arg(long, default_value_t = 25, value_parser = limit)]
    pub limit: usize,
    /// Resume from a previous page cursor.
    #[arg(long)]
    pub after: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Search the text of stored posts and comments.
    Search {
        /// SQLite full-text search query.
        query: String,
        /// Restrict results to a subreddit.
        #[arg(short, long)]
        sub: Option<String>,
        /// Restrict results to a username.
        #[arg(long)]
        author: Option<String>,
        /// Restrict results to posts or comments.
        #[arg(long, value_enum)]
        kind: Option<CacheKind>,
        /// Maximum number of items to return.
        #[arg(long, default_value_t = 25, value_parser = limit)]
        limit: usize,
    },
    /// Show counts of cached responses and stored items.
    Stats,
    /// Remove old responses and items.
    Prune {
        /// Remove entries older than this duration.
        #[arg(long)]
        older_than: Option<String>,
    },
    /// Delete the local store and cached token.
    Clear,
}

#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
    /// List the available skills.
    List,
    /// Print the requested skill.
    Get {
        /// Skill name.
        name: String,
    },
}

fn limit(raw: &str) -> Result<usize, &'static str> {
    raw.parse()
        .ok()
        .filter(|value| (1..=1000).contains(value))
        .ok_or("limit must be between 1 and 1000")
}
