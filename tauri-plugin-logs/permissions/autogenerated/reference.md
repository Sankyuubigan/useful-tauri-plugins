## Default Permission

Default permissions for the logs plugin.

#### This default permission set includes the following:

- `allow-get-last-logs-path`
- `allow-get-log-file-path`
- `allow-log-frontend-event`
- `allow-track-event`
- `allow-track-error`
- `allow-set-reporting-enabled`
- `allow-save-logs-file`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`logs:allow-get-last-logs-path`

</td>
<td>

Enables the get_last_logs_path command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:deny-get-last-logs-path`

</td>
<td>

Denies the get_last_logs_path command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:allow-get-log-file-path`

</td>
<td>

Enables the get_log_file_path command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:deny-get-log-file-path`

</td>
<td>

Denies the get_log_file_path command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:allow-log-frontend-event`

</td>
<td>

Enables the log_frontend_event command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:deny-log-frontend-event`

</td>
<td>

Denies the log_frontend_event command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:allow-save-logs-file`

</td>
<td>

Enables the save_logs_file command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:deny-save-logs-file`

</td>
<td>

Denies the save_logs_file command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:allow-set-reporting-enabled`

</td>
<td>

Enables the set_reporting_enabled command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:deny-set-reporting-enabled`

</td>
<td>

Denies the set_reporting_enabled command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:allow-track-error`

</td>
<td>

Enables the track_error command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:deny-track-error`

</td>
<td>

Denies the track_error command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:allow-track-event`

</td>
<td>

Enables the track_event command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:deny-track-event`

</td>
<td>

Denies the track_event command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`logs:allow-get-last-logs-path`

</td>
<td>

Allow reading the resolved dev mirror path (test/last_logs.txt).

</td>
</tr>

<tr>
<td>

`logs:allow-get-log-file-path`

</td>
<td>

Allow reading the resolved app log file path (king_orch.log).

</td>
</tr>

<tr>
<td>

`logs:allow-log-frontend-event`

</td>
<td>

Allow writing a frontend log line through the unified logger.

</td>
</tr>

<tr>
<td>

`logs:allow-track-event`

</td>
<td>

Allow sending an anonymized analytics event to the reporting backend.

</td>
</tr>

<tr>
<td>

`logs:allow-track-error`

</td>
<td>

Allow sending an error report to the reporting backend.

</td>
</tr>

<tr>
<td>

`logs:allow-set-reporting-enabled`

</td>
<td>

Allow toggling cloud error/analytics reporting at runtime.

</td>
</tr>

<tr>
<td>

`logs:allow-save-logs-file`

</td>
<td>

Allow writing the log text to a user-selected file path.

</td>
</tr>
</table>
