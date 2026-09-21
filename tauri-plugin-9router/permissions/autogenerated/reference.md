## Default Permission

Default permissions for the 9router plugin.

#### This default permission set includes the following:

- `allow-get-status`
- `allow-install-or-update`
- `allow-ensure-started`
- `allow-stop`
- `allow-set-router-dir`
- `allow-get-combos`
- `allow-open-dashboard`
- `allow-chat-completion`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`9router:allow-chat-completion`

</td>
<td>

Enables the chat_completion command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-chat-completion`

</td>
<td>

Denies the chat_completion command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-ensure-started`

</td>
<td>

Enables the ensure_started command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-ensure-started`

</td>
<td>

Denies the ensure_started command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-get-combos`

</td>
<td>

Enables the get_combos command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-get-combos`

</td>
<td>

Denies the get_combos command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-get-status`

</td>
<td>

Enables the get_status command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-get-status`

</td>
<td>

Denies the get_status command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-install-or-update`

</td>
<td>

Enables the install_or_update command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-install-or-update`

</td>
<td>

Denies the install_or_update command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-open-dashboard`

</td>
<td>

Enables the open_dashboard command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-open-dashboard`

</td>
<td>

Denies the open_dashboard command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-set-router-dir`

</td>
<td>

Enables the set_router_dir command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-set-router-dir`

</td>
<td>

Denies the set_router_dir command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-stop`

</td>
<td>

Enables the stop command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:deny-stop`

</td>
<td>

Denies the stop command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`9router:allow-get-status`

</td>
<td>

Allow reading the 9Router gateway status (installed/running/version/port).

</td>
</tr>

<tr>
<td>

`9router:allow-install-or-update`

</td>
<td>

Allow installing/updating 9Router (portable node.exe + npm bundle).

</td>
</tr>

<tr>
<td>

`9router:allow-ensure-started`

</td>
<td>

Allow lazily starting the 9Router server on demand.

</td>
</tr>

<tr>
<td>

`9router:allow-stop`

</td>
<td>

Allow stopping the 9Router server.

</td>
</tr>

<tr>
<td>

`9router:allow-set-router-dir`

</td>
<td>

Allow overriding the 9Router install directory and re-checking whether 9Router is present there.

</td>
</tr>

<tr>
<td>

`9router:allow-get-combos`

</td>
<td>

Allow listing 9Router LLM combos for the chat model picker.

</td>
</tr>

<tr>
<td>

`9router:allow-open-dashboard`

</td>
<td>

Allow opening the 9Router web dashboard in the default browser.

</td>
</tr>

<tr>
<td>

`9router:allow-chat-completion`

</td>
<td>

Allow running an OpenAI-compatible chat completion through 9Router.

</td>
</tr>
</table>
