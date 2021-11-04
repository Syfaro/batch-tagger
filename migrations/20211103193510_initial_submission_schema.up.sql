CREATE TABLE submission (
    site TEXT NOT NULL,
    id INTEGER NOT NULL,

    title TEXT NOT NULL,
    posted_at DATETIME NOT NULL,
    tags TEXT NOT NULL,

    PRIMARY KEY (site, id)
);
