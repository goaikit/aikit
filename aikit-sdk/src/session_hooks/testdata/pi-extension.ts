// aikit session hook my-hook; generated, do not edit
//
// Runs a program when a Pi session starts, is new or is resumed, and shows
// what it prints to the user as a notice. aikit replaces or removes this file
// as a whole; edits are lost.

const PROGRAM = "/opt/my tool/bin/tool";
const ARGS = ["managed", "apply", "--hook", "my-hook", "say \"hi\"\n"];
const REASONS = ["startup", "new", "resume"];

export default function (pi: any) {
  pi.on("session_start", async (event: any, ctx: any) => {
    if (!REASONS.includes(event?.reason)) return;
    try {
      const result = await pi.exec(PROGRAM, ARGS, { timeout: 10000 });
      const notice = String(result?.stdout ?? "").trim();
      if (notice && ctx?.hasUI) ctx.ui.notify(notice, "warning");
    } catch {
      // A missing or failing program never breaks the session.
    }
  });
}
