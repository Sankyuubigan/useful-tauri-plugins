## Default Permission

Default permissions for the cloud-routers plugin.

#### This default permission set includes the following:

- `allow-get-status`
- `allow-install-or-update`
- `allow-ensure-started`
- `allow-stop`
- `allow-set-router-dir`
- `allow-get-combos`
- `allow-set-api-key`
- `allow-open-dashboard`
- `allow-chat-completion`
- `allow-check-router-update`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`cloud-routers:allow-chat-completion`

</td>
<td>

Enables the chat_completion command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-chat-completion`

</td>
<td>

Denies the chat_completion command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-check-router-update`

</td>
<td>

Enables the check_router_update command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-check-router-update`

</td>
<td>

Denies the check_router_update command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-ensure-started`

</td>
<td>

Enables the ensure_started command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-ensure-started`

</td>
<td>

Denies the ensure_started command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-get-combos`

</td>
<td>

Enables the get_combos command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-get-combos`

</td>
<td>

Denies the get_combos command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-get-status`

</td>
<td>

Enables the get_status command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-get-status`

</td>
<td>

Denies the get_status command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-install-or-update`

</td>
<td>

Enables the install_or_update command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-install-or-update`

</td>
<td>

Denies the install_or_update command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-open-dashboard`

</td>
<td>

Enables the open_dashboard command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-open-dashboard`

</td>
<td>

Denies the open_dashboard command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-set-api-key`

</td>
<td>

Enables the set_api_key command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-set-api-key`

</td>
<td>

Denies the set_api_key command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-set-router-dir`

</td>
<td>

Enables the set_router_dir command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-set-router-dir`

</td>
<td>

Denies the set_router_dir command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-stop`

</td>
<td>

Enables the stop command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:deny-stop`

</td>
<td>

Denies the stop command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-get-status`

</td>
<td>

Allow reading the cloud router gateway status (installed/running/version/port).

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-install-or-update`

</td>
<td>

Allow installing/updating cloud routers (portable node.exe + npm bundle).

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-ensure-started`

</td>
<td>

Allow lazily starting the cloud router server on demand.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-stop`

</td>
<td>

Allow stopping the cloud router server.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-set-router-dir`

</td>
<td>

Allow overriding the cloud router install directory and re-checking whether the router is present there.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-get-combos`

</td>
<td>

Allow listing cloud router LLM combos for the chat model picker.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-open-dashboard`

</td>
<td>

Allow requesting the cloud router web dashboard (the host shows it as an in-app tab).

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-chat-completion`

</td>
<td>

Allow running an OpenAI-compatible chat completion through cloud routers.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-set-api-key`

</td>
<td>

Allow setting the cloud router API key.

</td>
</tr>

<tr>
<td>

`cloud-routers:allow-check-router-update`

</td>
<td>

Allow checking for cloud router updates from npm registry.

</td>
</tr>
</table>
