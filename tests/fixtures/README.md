# Fixtures

Real response bodies captured 2026-11-24 (no tokens). Serve them from the
`tiny_http` loopback server in `tests/support/`.

live/ (oauth.reddit.com, all with `raw_json=1`)
- sub-rust-hot: /r/rust/hot?limit=3
- sub-rust-new: /r/rust/new?limit=10
- sub-askreddit-top: /r/AskReddit/top?t=week&limit=1
- about-rust: /r/rust/about
- search-global: /search?q=ratatui&limit=3&sort=relevance&t=all
- search-rust: /r/rust/search?q=async&restrict_sr=true&limit=3
- search-subs: /subreddits/search?q=rust&limit=2
- search-users: /users/search?q=spez&limit=2
- user-spez-about: /user/spez/about
- user-spez-overview: /user/spez/overview?limit=4
- thread-big: /comments/<AskReddit top post>?limit=40&depth=3 (41 `more` stubs, 6 "continue this thread")
- morechildren: /api/morechildren for 20 of thread-big's hidden IDs
- thread-small: a complete small r/rust thread
- token.json / token-error.json: synthetic token endpoint bodies (fake values)

archive/ (arctic-shift.photon-reddit.com/api)
- posts-sub-rust: /posts/search?subreddit=rust&limit=3&sort=desc
- posts-search-rust: /posts/search?subreddit=rust&query=async&after=30d&limit=3
- posts-author-spez, comments-author-spez: author searches
- comments-search-rust: /comments/search?subreddit=rust&body=lifetime&limit=3
- subreddit-rust, subreddits-prefix, user-spez, users-prefix
- post-ids + comments-tree: one r/rust thread (posts/ids + comments/tree)
- error-timeout (HTTP 422), error-body-needs-filter (HTTP 400)
