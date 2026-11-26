import { useEffect, useState } from "react";
import {
  listTorrentIndexers,
  openRuTrackerLogin,
  setTorrentIndexerEnabled,
  testTorrentIndexer,
  type TorrentIndexer,
  type TorrentIndexerTest,
} from "@/shared/lib/audio.ts";
import styles from "./settings.module.css";

function testStatus(result: TorrentIndexerTest): string {
  if (result.error) return result.error;
  if (result.hits === 0) return "Reachable · No sample results";
  return `Reachable · ${result.hits} sample result${result.hits === 1 ? "" : "s"}`;
}

export function TorrentIndexerSettings() {
  const [indexers, setIndexers] = useState<TorrentIndexer[]>();
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [checks, setChecks] = useState<Partial<Record<TorrentIndexer["id"], string>>>({});
  const [testing, setTesting] = useState<TorrentIndexer["id"]>();
  const [openingLogin, setOpeningLogin] = useState(false);

  useEffect(() => {
    let current = true;
    listTorrentIndexers().then((indexers) => {
      if (current) setIndexers(indexers);
    }).catch((reason) => {
      if (current) setError(String(reason));
    });
    return () => { current = false; };
  }, [loadAttempt]);

  async function toggle(indexer: TorrentIndexer, enabled: boolean) {
    setSaving(true);
    setError(undefined);
    try {
      setIndexers(await setTorrentIndexerEnabled(indexer.id, enabled));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSaving(false);
    }
  }

  async function test(id: TorrentIndexer["id"]) {
    setTesting(id);
    try {
      const result = await testTorrentIndexer(id);
      setChecks((checks) => ({ ...checks, [id]: testStatus(result) }));
    } catch (reason) {
      setChecks((checks) => ({ ...checks, [id]: String(reason) }));
    } finally {
      setTesting(undefined);
    }
  }

  async function signIn() {
    setOpeningLogin(true);
    try {
      await openRuTrackerLogin();
      setChecks((checks) => ({ ...checks, ru_tracker: "After signing in, close the RuTracker window and click Test." }));
    } catch (reason) {
      setChecks((checks) => ({ ...checks, ru_tracker: String(reason) }));
    } finally {
      setOpeningLogin(false);
    }
  }

  return (
    <section className={styles.section} aria-labelledby="torrent-indexer-heading">
      <h2 id="torrent-indexer-heading">Torrent indexers</h2>
      {error && <p role="alert">{error}</p>}
      {!indexers && (error ? (
        <button className={styles.actionButton} type="button" onClick={() => {
          setError(undefined);
          setLoadAttempt((attempt) => attempt + 1);
        }}>Retry</button>
      ) : <p role="status" className={styles.mutedText}>Loading indexers…</p>)}
      {indexers?.every((indexer) => !indexer.enabled) &&
        <p className={styles.mutedText}>Torrent discovery is off.</p>}
      {indexers?.map((indexer) => (
        <div key={indexer.id} className={styles.settingRow}>
          <div className={styles.libraryInfo}>
            <label htmlFor={`torrent-indexer-${indexer.id}`}>{indexer.name}</label>
            <span className={styles.mutedText} role="status">
              {testing === indexer.id ? "Testing…" : checks[indexer.id] ?? (indexer.id === "ru_tracker" ? "Requires a RuTracker account" : "Built-in")}
            </span>
          </div>
          <div className={styles.indexerActions}>
            {indexer.id === "ru_tracker" && (
              <button className={styles.actionButton} type="button" disabled={openingLogin || testing !== undefined} onClick={() => void signIn()}>
                Sign in
              </button>
            )}
            <input
              id={`torrent-indexer-${indexer.id}`}
              type="checkbox"
              checked={indexer.enabled}
              disabled={saving}
              onChange={(event) => void toggle(indexer, event.target.checked)}
            />
            <button
              className={styles.actionButton}
              type="button"
              disabled={testing !== undefined}
              aria-label={`Test ${indexer.name} connection`}
              onClick={() => void test(indexer.id)}
            >
              Test
            </button>
          </div>
        </div>
      ))}
    </section>
  );
}
