// lib.rs - Student Time Management Smart Contract (Soroban / Stellar)

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    Address, Env, String, Vec,
    log,
};

// =====================
//  DATA STRUCTURES
// =====================

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum TaskStatus {
    Todo,
    InProgress,
    Completed,
    Overdue,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Task {
    pub id: u64,
    pub student: Address,
    pub title: String,
    pub description: String,
    pub subject: String,         // e.g. "Mathematics", "Computer Science"
    pub priority: TaskPriority,
    pub status: TaskStatus,
    pub deadline: u64,           // unix timestamp
    pub created_at: u64,
    pub completed_at: u64,       // 0 if not completed
    pub estimated_hours: u32,    // estimated study hours
    pub actual_hours: u32,       // actual hours spent
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Schedule {
    pub id: u64,
    pub student: Address,
    pub title: String,
    pub subject: String,
    pub start_time: u64,         // unix timestamp
    pub end_time: u64,
    pub is_recurring: bool,
    pub location: String,        // e.g. "Room 101", "Online"
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct StudySession {
    pub id: u64,
    pub student: Address,
    pub subject: String,
    pub task_id: u64,            // 0 if not linked to a task
    pub start_time: u64,
    pub end_time: u64,           // 0 if still ongoing
    pub duration_minutes: u32,   // filled when session ends
    pub notes: String,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct StudentStats {
    pub student: Address,
    pub total_tasks: u32,
    pub completed_tasks: u32,
    pub overdue_tasks: u32,
    pub total_study_minutes: u32,
    pub total_sessions: u32,
    pub last_active: u64,
}

// Storage keys
#[contracttype]
pub enum DataKey {
    Admin,
    TaskCount,
    ScheduleCount,
    SessionCount,
    Task(u64),
    Schedule(u64),
    Session(u64),
    StudentTasks(Address),
    StudentSchedules(Address),
    StudentSessions(Address),
    StudentStats(Address),
}

// =====================
//  CONTRACT
// =====================

#[contract]
pub struct StudentTimeManagement;

#[contractimpl]
impl StudentTimeManagement {

    // =====================
    //  INIT
    // =====================

    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TaskCount, &0u64);
        env.storage().instance().set(&DataKey::ScheduleCount, &0u64);
        env.storage().instance().set(&DataKey::SessionCount, &0u64);
        log!(&env, "Contract initialized. Admin: {}", admin);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    // =====================
    //  TASK MANAGEMENT
    // =====================

    /// Create a new task
    pub fn create_task(
        env: Env,
        student: Address,
        title: String,
        description: String,
        subject: String,
        priority: TaskPriority,
        deadline: u64,
        estimated_hours: u32,
    ) -> u64 {
        student.require_auth();

        let now = env.ledger().timestamp();
        if deadline <= now {
            panic!("Deadline must be in the future");
        }
        if estimated_hours == 0 {
            panic!("Estimated hours must be greater than 0");
        }

        let task_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TaskCount)
            .unwrap_or(0);

        let new_id = task_count + 1;

        let task = Task {
            id: new_id,
            student: student.clone(),
            title,
            description,
            subject,
            priority,
            status: TaskStatus::Todo,
            deadline,
            created_at: now,
            completed_at: 0,
            estimated_hours,
            actual_hours: 0,
        };

        env.storage().persistent().set(&DataKey::Task(new_id), &task);
        env.storage().instance().set(&DataKey::TaskCount, &new_id);

        // Update student task list
        let mut student_tasks: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::StudentTasks(student.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        student_tasks.push_back(new_id);
        env.storage()
            .persistent()
            .set(&DataKey::StudentTasks(student.clone()), &student_tasks);

        // Update student stats
        Self::increment_total_tasks(&env, &student);

        log!(&env, "Task created. ID: {}", new_id);
        new_id
    }

    /// Update task status
    pub fn update_task_status(
        env: Env,
        student: Address,
        task_id: u64,
        new_status: TaskStatus,
        actual_hours: u32,
    ) {
        student.require_auth();

        let mut task: Task = Self::get_task_or_panic(&env, task_id);

        if task.student != student {
            panic!("Only the task owner can update status");
        }
        if task.status == TaskStatus::Completed {
            panic!("Task is already completed");
        }

        let now = env.ledger().timestamp();

        // Auto-detect overdue
        let resolved_status = if new_status == TaskStatus::Completed {
            task.completed_at = now;
            task.actual_hours = actual_hours;
            // Update stats
            Self::increment_completed_tasks(&env, &student);
            TaskStatus::Completed
        } else if now > task.deadline && new_status != TaskStatus::Completed {
            Self::increment_overdue_tasks(&env, &student);
            TaskStatus::Overdue
        } else {
            new_status
        };

        task.status = resolved_status;
        env.storage().persistent().set(&DataKey::Task(task_id), &task);
        log!(&env, "Task {} status updated", task_id);
    }

    /// Update task details (only owner)
    pub fn update_task(
        env: Env,
        student: Address,
        task_id: u64,
        title: Option<String>,
        description: Option<String>,
        priority: Option<TaskPriority>,
        deadline: Option<u64>,
        estimated_hours: Option<u32>,
    ) {
        student.require_auth();

        let mut task: Task = Self::get_task_or_panic(&env, task_id);
        if task.student != student {
            panic!("Only the task owner can update it");
        }

        let now = env.ledger().timestamp();

        if let Some(t) = title { task.title = t; }
        if let Some(d) = description { task.description = d; }
        if let Some(p) = priority { task.priority = p; }
        if let Some(dl) = deadline {
            if dl <= now { panic!("Deadline must be in the future"); }
            task.deadline = dl;
        }
        if let Some(eh) = estimated_hours {
            if eh == 0 { panic!("Estimated hours must be greater than 0"); }
            task.estimated_hours = eh;
        }

        env.storage().persistent().set(&DataKey::Task(task_id), &task);
    }

    /// Delete a task (only owner or admin)
    pub fn delete_task(env: Env, caller: Address, task_id: u64) {
        caller.require_auth();

        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let task: Task = Self::get_task_or_panic(&env, task_id);

        if caller != admin && caller != task.student {
            panic!("Only task owner or admin can delete");
        }

        env.storage().persistent().remove(&DataKey::Task(task_id));
        log!(&env, "Task {} deleted", task_id);
    }

    // =====================
    //  SCHEDULE MANAGEMENT
    // =====================

    /// Create a schedule entry (class, exam, meeting, etc.)
    pub fn create_schedule(
        env: Env,
        student: Address,
        title: String,
        subject: String,
        start_time: u64,
        end_time: u64,
        is_recurring: bool,
        location: String,
    ) -> u64 {
        student.require_auth();

        if end_time <= start_time {
            panic!("End time must be after start time");
        }

        let schedule_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ScheduleCount)
            .unwrap_or(0);

        let new_id = schedule_count + 1;

        let schedule = Schedule {
            id: new_id,
            student: student.clone(),
            title,
            subject,
            start_time,
            end_time,
            is_recurring,
            location,
            created_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Schedule(new_id), &schedule);
        env.storage()
            .instance()
            .set(&DataKey::ScheduleCount, &new_id);

        // Update student schedule list
        let mut student_schedules: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::StudentSchedules(student.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        student_schedules.push_back(new_id);
        env.storage()
            .persistent()
            .set(&DataKey::StudentSchedules(student.clone()), &student_schedules);

        log!(&env, "Schedule created. ID: {}", new_id);
        new_id
    }

    /// Delete a schedule entry
    pub fn delete_schedule(env: Env, student: Address, schedule_id: u64) {
        student.require_auth();

        let schedule: Schedule = env
            .storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
            .unwrap_or_else(|| panic!("Schedule not found"));

        if schedule.student != student {
            panic!("Only the schedule owner can delete it");
        }

        env.storage()
            .persistent()
            .remove(&DataKey::Schedule(schedule_id));
        log!(&env, "Schedule {} deleted", schedule_id);
    }

    // =====================
    //  STUDY SESSION
    // =====================

    /// Start a study session
    pub fn start_session(
        env: Env,
        student: Address,
        subject: String,
        task_id: u64,
        notes: String,
    ) -> u64 {
        student.require_auth();

        let session_count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SessionCount)
            .unwrap_or(0);

        let new_id = session_count + 1;
        let now = env.ledger().timestamp();

        let session = StudySession {
            id: new_id,
            student: student.clone(),
            subject,
            task_id,
            start_time: now,
            end_time: 0,
            duration_minutes: 0,
            notes,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Session(new_id), &session);
        env.storage()
            .instance()
            .set(&DataKey::SessionCount, &new_id);

        // Update student session list
        let mut student_sessions: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::StudentSessions(student.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        student_sessions.push_back(new_id);
        env.storage()
            .persistent()
            .set(&DataKey::StudentSessions(student.clone()), &student_sessions);

        log!(&env, "Study session {} started", new_id);
        new_id
    }

    /// End a study session
    pub fn end_session(env: Env, student: Address, session_id: u64) -> u32 {
        student.require_auth();

        let mut session: StudySession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic!("Session not found"));

        if session.student != student {
            panic!("Only the session owner can end it");
        }
        if session.end_time != 0 {
            panic!("Session already ended");
        }

        let now = env.ledger().timestamp();
        let duration_minutes = ((now - session.start_time) / 60) as u32;

        session.end_time = now;
        session.duration_minutes = duration_minutes;

        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);

        // Update student stats
        Self::add_study_minutes(&env, &student, duration_minutes);

        log!(&env, "Session {} ended. Duration: {} min", session_id, duration_minutes);
        duration_minutes
    }

    // =====================
    //  QUERIES
    // =====================

    pub fn get_task(env: Env, task_id: u64) -> Task {
        Self::get_task_or_panic(&env, task_id)
    }

    pub fn get_task_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::TaskCount).unwrap_or(0)
    }

    pub fn get_schedule(env: Env, schedule_id: u64) -> Schedule {
        env.storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
            .unwrap_or_else(|| panic!("Schedule not found"))
    }

    pub fn get_session(env: Env, session_id: u64) -> StudySession {
        env.storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic!("Session not found"))
    }

    pub fn get_student_tasks(env: Env, student: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::StudentTasks(student))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_student_schedules(env: Env, student: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::StudentSchedules(student))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_student_sessions(env: Env, student: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::StudentSessions(student))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_student_stats(env: Env, student: Address) -> StudentStats {
        env.storage()
            .persistent()
            .get(&DataKey::StudentStats(student.clone()))
            .unwrap_or_else(|| StudentStats {
                student,
                total_tasks: 0,
                completed_tasks: 0,
                overdue_tasks: 0,
                total_study_minutes: 0,
                total_sessions: 0,
                last_active: 0,
            })
    }

    /// Calculate completion rate (0-100)
    pub fn get_completion_rate(env: Env, student: Address) -> u32 {
        let stats: StudentStats = env
            .storage()
            .persistent()
            .get(&DataKey::StudentStats(student.clone()))
            .unwrap_or_else(|| StudentStats {
                student,
                total_tasks: 0,
                completed_tasks: 0,
                overdue_tasks: 0,
                total_study_minutes: 0,
                total_sessions: 0,
                last_active: 0,
            });

        if stats.total_tasks == 0 {
            return 0;
        }

        (stats.completed_tasks * 100) / stats.total_tasks
    }

    // =====================
    //  PRIVATE HELPERS
    // =====================

    fn get_task_or_panic(env: &Env, task_id: u64) -> Task {
        env.storage()
            .persistent()
            .get(&DataKey::Task(task_id))
            .unwrap_or_else(|| panic!("Task not found"))
    }

    fn get_or_init_stats(env: &Env, student: &Address) -> StudentStats {
        env.storage()
            .persistent()
            .get(&DataKey::StudentStats(student.clone()))
            .unwrap_or_else(|| StudentStats {
                student: student.clone(),
                total_tasks: 0,
                completed_tasks: 0,
                overdue_tasks: 0,
                total_study_minutes: 0,
                total_sessions: 0,
                last_active: 0,
            })
    }

    fn increment_total_tasks(env: &Env, student: &Address) {
        let mut stats = Self::get_or_init_stats(env, student);
        stats.total_tasks += 1;
        stats.last_active = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::StudentStats(student.clone()), &stats);
    }

    fn increment_completed_tasks(env: &Env, student: &Address) {
        let mut stats = Self::get_or_init_stats(env, student);
        stats.completed_tasks += 1;
        stats.last_active = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::StudentStats(student.clone()), &stats);
    }

    fn increment_overdue_tasks(env: &Env, student: &Address) {
        let mut stats = Self::get_or_init_stats(env, student);
        stats.overdue_tasks += 1;
        env.storage()
            .persistent()
            .set(&DataKey::StudentStats(student.clone()), &stats);
    }

    fn add_study_minutes(env: &Env, student: &Address, minutes: u32) {
        let mut stats = Self::get_or_init_stats(env, student);
        stats.total_study_minutes += minutes;
        stats.total_sessions += 1;
        stats.last_active = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::StudentStats(student.clone()), &stats);
    }
}

// =====================
//  TESTS
// =====================

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env, String};

    #[test]
    fn test_full_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, StudentTimeManagement);
        let client = StudentTimeManagementClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let student = Address::generate(&env);

        // Init
        client.initialize(&admin);
        assert_eq!(client.get_admin(), admin);

        // Create task
        let future_deadline = env.ledger().timestamp() + 86400; // +1 day
        let task_id = client.create_task(
            &student,
            &String::from_str(&env, "Finish Soroban Assignment"),
            &String::from_str(&env, "Complete the smart contract"),
            &String::from_str(&env, "Blockchain"),
            &TaskPriority::High,
            &future_deadline,
            &5u32,
        );
        assert_eq!(task_id, 1);

        // Create schedule
        let start = env.ledger().timestamp() + 3600;
        let end = start + 5400; // 1.5 hours
        let schedule_id = client.create_schedule(
            &student,
            &String::from_str(&env, "Blockchain Lecture"),
            &String::from_str(&env, "Blockchain"),
            &start,
            &end,
            &false,
            &String::from_str(&env, "Room 204"),
        );
        assert_eq!(schedule_id, 1);

        // Start study session
        let session_id = client.start_session(
            &student,
            &String::from_str(&env, "Blockchain"),
            &task_id,
            &String::from_str(&env, "Studying Soroban docs"),
        );
        assert_eq!(session_id, 1);

        // End session
        env.ledger().with_mut(|li| li.timestamp += 3600); // fast-forward 1 hour
        let duration = client.end_session(&student, &session_id);
        assert_eq!(duration, 60u32); // 60 minutes

        // Complete task
        client.update_task_status(
            &student,
            &task_id,
            &TaskStatus::Completed,
            &5u32,
        );

        // Check stats
        let stats = client.get_student_stats(&student);
        assert_eq!(stats.total_tasks, 1);
        assert_eq!(stats.completed_tasks, 1);
        assert_eq!(stats.total_study_minutes, 60);
        assert_eq!(stats.total_sessions, 1);

        // Check completion rate
        let rate = client.get_completion_rate(&student);
        assert_eq!(rate, 100u32);
    }

    #[test]
    #[should_panic(expected = "Deadline must be in the future")]
    fn test_past_deadline() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, StudentTimeManagement);
        let client = StudentTimeManagementClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let student = Address::generate(&env);

        client.initialize(&admin);

        // Deadline in the past — should panic
        client.create_task(
            &student,
            &String::from_str(&env, "Late Task"),
            &String::from_str(&env, "Already expired"),
            &String::from_str(&env, "Math"),
            &TaskPriority::Low,
            &0u64, // past timestamp
            &2u32,
        );
    }

    #[test]
    #[should_panic(expected = "Session already ended")]
    fn test_double_end_session() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, StudentTimeManagement);
        let client = StudentTimeManagementClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let student = Address::generate(&env);

        client.initialize(&admin);
        let session_id = client.start_session(
            &student,
            &String::from_str(&env, "Math"),
            &0u64,
            &String::from_str(&env, ""),
        );
        env.ledger().with_mut(|li| li.timestamp += 1800);
        client.end_session(&student, &session_id);
        client.end_session(&student, &session_id); // should panic
    }
}