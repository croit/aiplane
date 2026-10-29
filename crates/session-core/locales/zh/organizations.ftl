organization-error-invalid = 请检查名称和成员组，然后提交修正后的表单。
organization-error-conflict = 组织已发生更改。请重新加载当前设置后再保存。
organization-error-not-found = 此组织不可用。请检查地址，或请管理员检查你的访问权限。
organization-error-permission-required = 你需要是组织成员并拥有代理管理权限。请管理员检查你的所属组。
organization-error-active-limit = 组织已达到启用智能体的数量上限。请停用一个智能体，或请管理员提高上限。
organization-error-name-conflict = 此组织已有同名智能体。请选择其他名称。
organization-error-admin-required = 只有管理员可以管理组织设置。请管理员进行此更改。
organization-error-execution-unavailable = 此团队智能体 或其运行权限不可用。重试前请检查其启用状态、权限和私有对话。
organizations-heading = 组织
organizations-empty = 你没有可访问的组织。
organization-create = 创建组织
organization-edit = 编辑组织
organization-member-groups = 成员组
organization-member-group-count = { $count ->
    [one] { $group } · { $count } 位用户
   *[other] { $group } · { $count } 位用户
}
organization-membership-explanation = 组成员身份提供组织访问权限。智能体共享权限和工具授权需要单独配置。
organization-no-members = 此组织尚未配置成员组。
organization-reload = 重新加载组织
organization-saved = 组织已保存。
organization-dots-empty = 此组织中没有与你共享的智能体。
organization-active-count = 已激活 { $count } 个智能体，上限 { $limit } 个
organization-owned-by = 组织：{ $name }
organization-configuration-only = 拥有写入权限的成员可以配置此团队智能体，与其已保存的草稿或已发布版本进行私密对话，并通过定时任务和 Webhook 自动运行选定的已发布版本。智能体使用自己的权限和所选的已连接账户。
organization-share-search-truncated = 搜索已达到上限。请输入更具体的名称或完整的电子邮件地址。
