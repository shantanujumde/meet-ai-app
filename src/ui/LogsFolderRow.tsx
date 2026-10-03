/**
 * Settings' "Open logs folder" row (TUR-46): the folder with `meet-ai.log` and
 * any crash files, so a user who hits a bug has one place to look and one file
 * to attach. The app never sends these anywhere itself.
 */

import { useState } from "react";
import { openLogsFolder } from "@/ipc/client";
import type { UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { Button, Row, RowLabel } from "@/ui/primitives";
import { ErrorState } from "@/ui/states";

export function LogsFolderRow() {
  // The folder may fail to open (no file manager, a removed volume); the rest
  // of Settings is fine, so the message goes under the row.
  const [error, setError] = useState<UiError | null>(null);

  const open = async () => {
    setError(null);
    try {
      await openLogsFolder();
    } catch (thrown) {
      setError(toUiError(thrown));
    }
  };

  return (
    <Row stacked>
      <div className="flex justify-between gap-5">
        <RowLabel
          name="Logs"
          detail="The app's log and crash files, to attach to a bug report"
          mono={false}
        />
        <Button size="small" onClick={() => void open()}>
          Open logs folder
        </Button>
      </div>
      {error ? <ErrorState error={error} /> : null}
    </Row>
  );
}
