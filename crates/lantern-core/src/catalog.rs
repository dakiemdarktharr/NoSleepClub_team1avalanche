//! Bounded catalogue queries and cheap local change detection.
use super::*;

impl Store {
    pub fn company_count(&self, query: &str, watched: bool) -> Result<u64> {
        let conn = lock(&self.conn)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM companies WHERE instr(search_text,?1)>0 AND (?2=0 OR watched=1)",
            params![normalize_search(query), watched],
            |r| r.get(0),
        )?;
        Ok(count as u64)
    }
    pub fn company_page(
        &self,
        query: &str,
        watched: bool,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Company>> {
        let conn = lock(&self.conn)?;
        let mut stmt = conn.prepare("SELECT payload FROM companies WHERE instr(search_text,?1)>0 AND (?2=0 OR watched=1) ORDER BY watched DESC,search_text,id LIMIT ?3 OFFSET ?4")?;
        let rows = stmt.query_map(
            params![
                normalize_search(query),
                watched,
                limit.clamp(1, 200),
                offset
            ],
            |r| r.get::<_, String>(0),
        )?;
        rows.map(|r| decode(r?)).collect()
    }
    /// Monotonic for mutations on this Store connection. No private data leaves the server.
    pub fn local_revision(&self) -> Result<u64> {
        Ok(lock(&self.conn)?.total_changes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_are_bounded_and_watchlist_is_not_cut_off_by_first_page() -> Result<()> {
        let store = Store::memory()?;
        for n in 0..205 {
            store.save_company(&Company {
                id: format!("fixture-{n}"),
                name: format!("Công ty {n:03}"),
                tax_id: None,
                ticker: None,
                sector: None,
                location: None,
                website: None,
                source_url: None,
                description: None,
                identity_status: "candidate_unverified".into(),
                watched: n == 204,
                updated_at: now(),
            })?;
        }
        assert_eq!(store.company_count("cong ty", false)?, 205);
        assert_eq!(store.company_page("cong ty", false, 100, 200)?.len(), 5);
        assert_eq!(store.company_page("", true, 100, 0)?[0].id, "fixture-204");
        let before = store.local_revision()?;
        store.set_watch("fixture-203", true)?;
        assert!(store.local_revision()? > before);
        assert_eq!(store.company_count("", true)?, 2);
        Ok(())
    }
}
