use std::fmt::Write;
use std::path::Path;
use std::process::Command;

use nu_plugin::{serve_plugin, MsgPackSerializer, Plugin, PluginCommand};
use nu_plugin::{EngineInterface, EvaluatedCall, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Value};

use git2::{BranchType, Repository, Status, StatusOptions};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct GitPromptPlugin;

impl Plugin for GitPromptPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        vec![Box::new(GitPrompt)]
    }
}

pub struct GitPrompt;

impl SimplePluginCommand for GitPrompt {
    type Plugin = GitPromptPlugin;

    fn name(&self) -> &str {
        "git_prompt"
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self)).category(Category::Experimental)
    }

    fn description(&self) -> &str {
        "One line git status output to show in your nushell prompt"
    }

    fn examples(&self) -> Vec<Example> {
        vec![]
    }

    fn run(
        &self,
        _plugin: &GitPromptPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let current_dir = if let Ok(current_dir) = engine.get_current_dir() {
            current_dir
        } else {
            return Ok(Value::string("", call.head));
        };

        let path_current_dir = Path::new(&current_dir);

        let current_dir_exists = path_current_dir.is_dir();
        if !current_dir_exists {
            return Ok(Value::string("", call.head));
        }

        let git_dir = path_current_dir.join(".git");
        if git_dir.is_dir() {
            let mut size: u64 = 0;
            for entry in WalkDir::new(git_dir).into_iter().flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        size += metadata.len();
                    }
                }
            }

            if size > 1_000_000_000 {
                return Ok(Value::string("", call.head));
            }
        }

        let git_status = if let Some(git_status) = GitStatus::init(&current_dir) {
            git_status
        } else {
            return Ok(Value::string("", call.head));
        };

        let mut v: Vec<String> = Vec::with_capacity(6);

        let remote = if !git_status.remote.is_empty() {
            "".to_string()
        } else {
            "".to_string()
        };

        let branch_tag = if git_status.tag.is_empty() {
            git_status.branch.clone()
        } else {
            git_status.tag.clone()
        };

        if !remote.is_empty() {
            v.push(remote);
        }

        if !branch_tag.is_empty() {
            v.push(branch_tag);
        }

        let green = git_status.get_green();
        if !green.is_empty() {
            v.push(green);
        }

        let yellow = git_status.get_yellow();
        if !yellow.is_empty() {
            v.push(yellow);
        }

        let gray = git_status.get_gray();
        if !gray.is_empty() {
            v.push(gray);
        }

        let red = git_status.get_red();
        if !red.is_empty() {
            v.push(red);
        }

        let formatted = format!(" {}", v.join(" ").trim());
        Ok(Value::string(formatted, call.head))
    }
}

#[derive(Debug)]
pub struct GitStatus {
    pub branch: String,
    pub tag: String,
    pub remote: String,

    pub index_new: u16,
    pub index_modified: u16,
    pub index_deleted: u16,
    pub index_renamed: u16,
    pub index_typechange: u16,

    pub wt_new: u16,
    pub wt_modified: u16,
    pub wt_deleted: u16,
    pub wt_renamed: u16,
    pub wt_typechange: u16,

    pub ignored: u16,
    pub conflicted: u16,
    pub ahead: u16,
    pub behind: u16,
}

impl GitStatus {
    pub fn init(repo_path: &str) -> Option<Self> {
        let repo = match Repository::open(repo_path) {
            Ok(repo) => repo,
            Err(_) => {
                return None;
            }
        };

        let mut index_new: u16 = 0;
        let mut index_modified: u16 = 0;
        let mut index_deleted: u16 = 0;
        let mut index_renamed: u16 = 0;
        let mut index_typechange: u16 = 0;

        let mut wt_new: u16 = 0;
        let mut wt_modified: u16 = 0;
        let mut wt_deleted: u16 = 0;
        let mut wt_renamed: u16 = 0;
        let mut wt_typechange: u16 = 0;

        let mut ignored: u16 = 0;
        let mut conflicted: u16 = 0;
        let mut ahead: u16 = 0;
        let mut behind: u16 = 0;

        let mut remote = String::new();

        let branch = match repo.head() {
            Ok(reference) => {
                if let Some(name) = reference.shorthand() {
                    if name == "HEAD" {
                        if let Ok(commit) = reference.peel_to_commit() {
                            let mut id = String::new();
                            for byte in &commit.id().as_bytes()[..4] {
                                write!(&mut id, "{byte:x}").unwrap();
                            }
                            id
                        } else {
                            "HEAD".to_string()
                        }
                    } else {
                        let branch = name.to_string();

                        remote = if let Ok(branch) = repo.find_branch(&branch, BranchType::Local) {
                            if let Ok(upstream) = branch.upstream() {
                                if let (Some(local), Some(upstream)) =
                                    (branch.get().target(), upstream.get().target())
                                {
                                    if let Ok((ahead_x, behind_x)) =
                                        repo.graph_ahead_behind(local, upstream)
                                    {
                                        ahead = ahead_x as u16;
                                        behind = behind_x as u16;
                                    }
                                }

                                if let Ok(Some(name)) = upstream.name() {
                                    name.to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };

                        branch
                    }
                } else {
                    "HEAD".to_string()
                }
            }
            Err(ref err) if err.code() == git2::ErrorCode::BareRepo => "master".to_string(),
            Err(_) if repo.is_empty().unwrap_or(false) => "master".to_string(),
            Err(_) => "HEAD".to_string(),
        };

        let mut tag = String::new();
        let output_result = Command::new("git")
            .args(["describe", "--tags", "--abbrev=0"])
            .current_dir(repo_path)
            .output();
        if let Ok(output) = output_result {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    tag = stdout.trim().to_string();
                }
            }
        }

        let mut status_options = StatusOptions::new();
        status_options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true);

        let statuses = match repo.statuses(Some(&mut status_options)) {
            Ok(statuses) => statuses,
            Err(_) => {
                return None;
            }
        };

        statuses.iter().for_each(|status_entry| {
            let status = status_entry.status();

            if status == Status::INDEX_NEW {
                index_new += 1;
            }

            if status == Status::INDEX_MODIFIED {
                index_modified += 1;
            }

            if status == Status::INDEX_DELETED {
                index_deleted += 1;
            }

            if status == Status::INDEX_RENAMED {
                index_renamed += 1;
            }

            if status == Status::INDEX_TYPECHANGE {
                index_typechange += 1;
            }

            if status == Status::WT_NEW {
                wt_new += 1;
            }

            if status == Status::WT_MODIFIED {
                wt_modified += 1;
            }

            if status == Status::WT_DELETED {
                wt_deleted += 1;
            }

            if status == Status::WT_RENAMED {
                wt_renamed += 1;
            }

            if status == Status::WT_TYPECHANGE {
                wt_typechange += 1;
            }

            if status == Status::IGNORED {
                ignored += 1;
            }

            if status == Status::CONFLICTED {
                conflicted += 1;
            }
        });

        Some(Self {
            branch,
            tag,
            remote,
            index_new,
            index_modified,
            index_deleted,
            index_renamed,
            index_typechange,
            wt_new,
            wt_modified,
            wt_deleted,
            wt_renamed,
            wt_typechange,
            ignored,
            conflicted,
            ahead,
            behind,
        })
    }

    pub fn get_green(&self) -> String {
        let mut greens: Vec<String> = Vec::with_capacity(4);

        if self.index_new > 0 {
            greens.push(format!("+{}", self.index_new));
        }

        if self.index_modified > 0 {
            greens.push(format!("+~{}", self.index_modified));
        }

        if self.index_renamed > 0 {
            greens.push(format!("+->{}", self.index_renamed));
        }

        if self.index_typechange > 0 {
            greens.push(format!("+t{}", self.index_typechange));
        }

        greens.join(" ")
    }

    pub fn get_yellow(&self) -> String {
        let mut yellow: Vec<String> = Vec::with_capacity(6);

        if self.wt_new > 0 {
            yellow.push(format!("?{}", self.wt_new));
        }

        if self.wt_modified > 0 {
            yellow.push(format!("~{}", self.wt_modified));
        }

        if self.wt_renamed > 0 {
            yellow.push(format!("->{}", self.wt_renamed));
        }

        if self.wt_typechange > 0 {
            yellow.push(format!("t{}", self.wt_typechange));
        }

        if self.ahead > 0 {
            yellow.push(format!("↑{}", self.ahead));
        }

        if self.behind > 0 {
            yellow.push(format!("↓{}", self.behind));
        }

        yellow.join(" ")
    }

    pub fn get_gray(&self) -> String {
        if self.ignored > 0 {
            return format!("!{}", self.ignored);
        }

        String::new()
    }

    pub fn get_red(&self) -> String {
        let mut red: Vec<String> = Vec::with_capacity(3);

        if self.index_deleted > 0 {
            red.push(format!("+-{}", self.index_deleted));
        }

        if self.wt_deleted > 0 {
            red.push(format!("-{}", self.wt_deleted));
        }

        if self.conflicted > 0 {
            red.push(format!("c{}", self.conflicted));
        }

        red.join(" ")
    }
}

fn main() {
    serve_plugin(&GitPromptPlugin, MsgPackSerializer);
}
