"""Independent read-only inspection of the disposable SDK journal copy."""
import json
import pathlib
import sqlite3
import sys

database = pathlib.Path(sys.argv[1]).resolve()
connection = sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)
try:
    connection.execute('BEGIN')
    def bodies(table):
        # Table identifiers below are fixed in this script, never supplied as data.
        return [json.loads(row[0]) for row in connection.execute(f'SELECT body FROM {table} ORDER BY rowid LIMIT 2')]
    print(json.dumps({
        'schema_version': connection.execute('PRAGMA user_version').fetchone()[0],
        'outgoing': bodies('outgoing_messages'),
        'append_checkpoints': bodies('message_append_checkpoints'),
        'handoffs': bodies('message_handoffs'),
        'qualified_observation_count': connection.execute('SELECT count(*) FROM message_observations').fetchone()[0],
    }, ensure_ascii=False, indent=2))
finally:
    connection.close()
